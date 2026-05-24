use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use std::collections::HashMap;

use crate::core::mercator::*;
use crate::ui::{ReloadWorldEvent, UiPreferences};
use crate::drone::SpawnDroneEvent;

pub struct MapTilePlugin;

impl Plugin for MapTilePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MapTileState>()
            .init_resource::<TileRetryQueue>()
            .add_systems(Update, poll_tile_downloads)
            .add_systems(Update, process_tile_retries);
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MapProvider {
    OpenStreetMap,
    EsriSatellite,
    EsriStreet,
    CartoDark,
    CartoLight,
}

impl MapProvider {
    fn url(&self, z: u32, x: u32, y: u32) -> String {
        match self {
            MapProvider::OpenStreetMap => {
                format!("https://tile.openstreetmap.org/{z}/{x}/{y}.png")
            }
            MapProvider::EsriSatellite => {
                format!("https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{z}/{y}/{x}")
            }
            MapProvider::EsriStreet => {
                format!("https://server.arcgisonline.com/ArcGIS/rest/services/World_Street_Map/MapServer/tile/{z}/{y}/{x}")
            }
            MapProvider::CartoDark => {
                format!("https://basemaps.cartocdn.com/dark_all/{z}/{x}/{y}.png")
            }
            MapProvider::CartoLight => {
                format!("https://basemaps.cartocdn.com/light_all/{z}/{x}/{y}.png")
            }
        }
    }

    fn name(&self) -> &'static str {
        match self {
            MapProvider::OpenStreetMap => "OpenStreetMap",
            MapProvider::EsriSatellite => "Satellite",
            MapProvider::EsriStreet => "Esri Streets",
            MapProvider::CartoDark => "Dark",
            MapProvider::CartoLight => "Light",
        }
    }

    fn file_ext(&self) -> &'static str {
        match self {
            MapProvider::OpenStreetMap | MapProvider::CartoDark | MapProvider::CartoLight => "png",
            MapProvider::EsriSatellite | MapProvider::EsriStreet => "jpg",
        }
    }
}

/// Tile state for the UI minimap panel.
///
/// Separate from `SatelliteTerrainState` (3D ground plane) because the minimap
/// uses a different provider system, zoom logic, and egui-based rendering.
#[derive(Resource)]
pub struct MapTileState {
    pub center_pixel_x: f64,
    pub center_pixel_y: f64,
    pub zoom: u32,
    pub tiles: HashMap<TileKey, TileState>,
    pub provider: MapProvider,
    pub show_grid: bool,
    pub show_labels: bool,
}

impl Default for MapTileState {
    fn default() -> Self {
        let lat = 37.7749;
        let lon = -122.4194;
        let z = 14;
        let (px, py) = lat_lon_to_pixel(lat, lon, z);
        Self {
            center_pixel_x: px,
            center_pixel_y: py,
            zoom: z,
            tiles: HashMap::new(),
            provider: MapProvider::EsriSatellite,
            show_grid: true,
            show_labels: true,
        }
    }
}

pub enum TileState {
    Loading,
    Ready(egui::ColorImage, Option<egui::TextureHandle>),
    Failed,
}

#[derive(Component)]
pub struct TileDownloadTask {
    pub task: bevy::tasks::Task<Result<Vec<u8>, String>>,
    pub key: TileKey,
    pub _provider: MapProvider,
}

fn tile_cache_path(provider: &MapProvider, z: u32, x: u32, y: u32) -> std::path::PathBuf {
    std::path::PathBuf::from(format!(
        "data/tiles/{}_{z}/{x}_{y}.{}",
        provider.name().to_lowercase().replace(" ", "_"),
        provider.file_ext()
    ))
}

fn download_tile(key: TileKey, provider: MapProvider) -> Result<Vec<u8>, String> {
    let cache_path = tile_cache_path(&provider, key.z, key.x, key.y);
    if cache_path.exists() {
        return std::fs::read(&cache_path).map_err(|e| format!("cache read: {e}"));
    }

    let url = provider.url(key.z, key.x, key.y);
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("client build: {e}"))?;
    let response = client
        .get(&url)
        .header("User-Agent", "drone-sim/1.0")
        .send()
        .map_err(|e| format!("download: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    let bytes = response.bytes().map_err(|e| format!("read bytes: {e}"))?.to_vec();

    if let Some(parent) = cache_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&cache_path, &bytes);

    Ok(bytes)
}

#[derive(Resource, Default)]
pub struct TileRetryQueue {
    pub retries: Vec<(TileKey, MapProvider, f32)>,
}

pub fn poll_tile_downloads(
    mut commands: Commands,
    mut tasks: Query<(Entity, &mut TileDownloadTask)>,
    mut tile_state: ResMut<MapTileState>,
    mut retry_queue: ResMut<TileRetryQueue>,
) {
    let mut success_count = 0usize;
    let mut fail_count = 0usize;

    for (entity, mut task) in tasks.iter_mut() {
        if let Some(result) =
            bevy::tasks::block_on(bevy::tasks::futures_lite::future::poll_once(&mut task.task))
        {
            match result {
                Ok(bytes) => {
                    match image::load_from_memory(&bytes) {
                        Ok(img) => {
                            let rgba = img.to_rgba8();
                            let size = [rgba.width() as usize, rgba.height() as usize];
                            let pixels = rgba.into_raw();
                            let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
                            tile_state.tiles.insert(task.key, TileState::Ready(color_image, None));
                            success_count += 1;
                        }
                        Err(e) => {
                            eprintln!("Minimap tile decode error for {:?}: {}", task.key, e);
                            tile_state.tiles.insert(task.key, TileState::Failed);
                            retry_queue.retries.push((task.key, task._provider, 2.0));
                            fail_count += 1;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Minimap tile download error for {:?}: {}", task.key, e);
                    tile_state.tiles.insert(task.key, TileState::Failed);
                    retry_queue.retries.push((task.key, task._provider, 2.0));
                    fail_count += 1;
                }
            }
            commands.entity(entity).despawn();
        }
    }

    if success_count > 0 || fail_count > 0 {
        let ready = tile_state.tiles.values().filter(|s| matches!(s, TileState::Ready(_, _))).count();
        let loading = tile_state.tiles.values().filter(|s| matches!(s, TileState::Loading)).count();
        let failed = tile_state.tiles.values().filter(|s| matches!(s, TileState::Failed)).count();
        println!("Minimap tiles: +{} success, +{} fail | total: {} ready, {} loading, {} failed",
            success_count, fail_count, ready, loading, failed);
    }
}

fn process_tile_retries(
    mut commands: Commands,
    time: Res<Time>,
    mut retry_queue: ResMut<TileRetryQueue>,
    mut tile_state: ResMut<MapTileState>,
) {
    let dt = time.delta_seconds();
    let mut to_retry = Vec::new();
    retry_queue.retries.retain_mut(|(key, provider, delay)| {
        *delay -= dt;
        if *delay <= 0.0 {
            to_retry.push((*key, *provider));
            false
        } else {
            true
        }
    });
    for (key, provider) in to_retry {
        if tile_state.tiles.get(&key).is_some() {
            tile_state.tiles.insert(key, TileState::Loading);
            let task = bevy::tasks::IoTaskPool::get().spawn(async move {
                download_tile(key, provider)
            });
            commands.spawn(TileDownloadTask { task, key, _provider: provider });
        }
    }
}

fn draw_lat_lon_grid(
    painter: &egui::Painter,
    rect: egui::Rect,
    tile_state: &MapTileState,
    zoom: u32,
) {
    let tl_px = tile_state.center_pixel_x - rect.width() as f64 / 2.0;
    let tl_py = tile_state.center_pixel_y - rect.height() as f64 / 2.0;

    let (lat_step, lon_step) = grid_spacing_for_zoom(zoom);

    let (top_lat, left_lon) = pixel_to_lat_lon(tl_px, tl_py, zoom);
    let (bottom_lat, right_lon) = pixel_to_lat_lon(
        tl_px + rect.width() as f64,
        tl_py + rect.height() as f64,
        zoom,
    );

    let grid_color = egui::Color32::from_rgba_unmultiplied(120, 140, 160, 80);
    let label_color = egui::Color32::from_rgba_unmultiplied(160, 180, 200, 200);
    let stroke = egui::Stroke::new(1.0, grid_color);

    let mut lon = (left_lon / lon_step).floor() * lon_step;
    while lon <= right_lon {
        let (px, _) = lat_lon_to_pixel(top_lat, lon, zoom);
        let screen_x = px - tl_px + rect.min.x as f64;

        if screen_x >= rect.min.x as f64 && screen_x <= rect.max.x as f64 {
            let top = egui::pos2(screen_x as f32, rect.min.y);
            let bottom = egui::pos2(screen_x as f32, rect.max.y);
            painter.line_segment([top, bottom], stroke);

            let label = format_lon(lon);
            painter.text(
                egui::pos2(screen_x as f32, rect.min.y + 2.0),
                egui::Align2::CENTER_TOP,
                &label,
                egui::FontId::proportional(9.0),
                label_color,
            );
        }
        lon += lon_step;
    }

    let mut lat = (bottom_lat / lat_step).floor() * lat_step;
    while lat <= top_lat {
        let (_, py) = lat_lon_to_pixel(lat, left_lon, zoom);
        let screen_y = py - tl_py + rect.min.y as f64;

        if screen_y >= rect.min.y as f64 && screen_y <= rect.max.y as f64 {
            let left = egui::pos2(rect.min.x, screen_y as f32);
            let right = egui::pos2(rect.max.x, screen_y as f32);
            painter.line_segment([left, right], stroke);

            let label = format_lat(lat);
            painter.text(
                egui::pos2(rect.min.x + 2.0, screen_y as f32),
                egui::Align2::LEFT_TOP,
                &label,
                egui::FontId::proportional(9.0),
                label_color,
            );
        }
        lat += lat_step;
    }
}

/// Determine grid line spacing based on zoom level
fn grid_spacing_for_zoom(zoom: u32) -> (f64, f64) {
    match zoom {
        0..=3 => (10.0, 10.0),
        4..=5 => (5.0, 5.0),
        6..=7 => (2.0, 2.0),
        8..=9 => (1.0, 1.0),
        10..=11 => (0.5, 0.5),
        12..=13 => (0.2, 0.2),
        14..=15 => (0.1, 0.1),
        16..=17 => (0.05, 0.05),
        18..=19 => (0.02, 0.02),
        _ => (0.01, 0.01),
    }
}

fn format_lat(lat: f64) -> String {
    let abs_lat = lat.abs();
    let dir = if lat >= 0.0 { "N" } else { "S" };
    format!("{:.4}°{}", abs_lat, dir)
}

fn format_lon(lon: f64) -> String {
    let abs_lon = lon.abs();
    let dir = if lon >= 0.0 { "E" } else { "W" };
    format!("{:.4}°{}", abs_lon, dir)
}

// ─── Map panel rendering ───

pub fn map_panel(
    mut commands: Commands,
    mut contexts: EguiContexts,
    mut tile_state: ResMut<MapTileState>,
    mut retry_queue: ResMut<TileRetryQueue>,
    mut input_state: ResMut<crate::drone::controller::DroneInputState>,
    mut drone_query: Query<(Entity, &crate::drone::DroneIdentity, &crate::drone::Kinematics, &mut crate::drone::MissionState, &mut crate::drone::FlightControl)>,
    geo: Res<crate::core::gps::GeoReference>,
    mut navigate_events: EventWriter<ReloadWorldEvent>,
    ui_prefs: Res<UiPreferences>,
    mut spawn_events: EventWriter<SpawnDroneEvent>,
) {
    egui::CentralPanel::default().show(contexts.ctx_mut(), |ui| {
            // ── Map style controls ──
            ui.horizontal(|ui| {
                ui.label("Style:");
                let providers = [
                    MapProvider::OpenStreetMap,
                    MapProvider::EsriSatellite,
                    MapProvider::EsriStreet,
                    MapProvider::CartoLight,
                    MapProvider::CartoDark,
                ];
                for provider in providers {
                    let selected = tile_state.provider == provider;
                    if ui.selectable_label(selected, provider.name()).clicked() && !selected {
                        tile_state.provider = provider;
                        tile_state.tiles.clear();
                        retry_queue.retries.clear();
                    }
                }
            });

            ui.horizontal(|ui| {
                ui.checkbox(&mut tile_state.show_grid, "Grid lines");
                ui.checkbox(&mut tile_state.show_labels, "Labels");
            });

            ui.separator();

            let available = ui.available_size();
            let (rect, response) = ui.allocate_exact_size(
                available,
                egui::Sense::click_and_drag(),
            );

            // ── Input handling ──
            let scroll = if response.hovered() {
                ui.input(|i| i.raw_scroll_delta.y)
            } else {
                0.0
            };

            if response.dragged() {
                let delta = response.drag_delta();
                tile_state.center_pixel_x -= delta.x as f64;
                tile_state.center_pixel_y -= delta.y as f64;
            }

            if scroll.abs() > 0.5 {
                let old_zoom = tile_state.zoom;
                let zoom_delta = if scroll.abs() > 5.0 { 2 } else { 1 };
                let new_zoom = (old_zoom as i32 + scroll.signum() as i32 * zoom_delta).clamp(1, 19) as u32;
                if new_zoom != old_zoom {
                    // Zoom around mouse cursor if hovering, otherwise around center
                    if let Some(mouse_px) = response.hover_pos() {
                        let cursor_offset_x = (mouse_px.x - rect.center().x) as f64;
                        let cursor_offset_y = (mouse_px.y - rect.center().y) as f64;
                        let cursor_px = tile_state.center_pixel_x + cursor_offset_x;
                        let cursor_py = tile_state.center_pixel_y + cursor_offset_y;
                        let (cursor_lat, cursor_lon) = pixel_to_lat_lon(cursor_px, cursor_py, old_zoom);
                        let (new_cursor_px, new_cursor_py) = lat_lon_to_pixel(cursor_lat, cursor_lon, new_zoom);
                        tile_state.center_pixel_x = new_cursor_px - cursor_offset_x;
                        tile_state.center_pixel_y = new_cursor_py - cursor_offset_y;
                    } else {
                        let (lat, lon) = pixel_to_lat_lon(
                            tile_state.center_pixel_x,
                            tile_state.center_pixel_y,
                            old_zoom,
                        );
                        let (px, py) = lat_lon_to_pixel(lat, lon, new_zoom);
                        tile_state.center_pixel_x = px;
                        tile_state.center_pixel_y = py;
                    }
                    tile_state.zoom = new_zoom;
                    // Clear old-zoom tiles so we fetch new ones
                    tile_state.tiles.retain(|k, _| k.z == new_zoom);
                    retry_queue.retries.clear();
                }
            }

            // ── Compute visible tiles ──
            let tl_px = tile_state.center_pixel_x - rect.width() as f64 / 2.0;
            let tl_py = tile_state.center_pixel_y - rect.height() as f64 / 2.0;
            let min_tx = (tl_px / 256.0).floor() as i32;
            let max_tx = ((tl_px + rect.width() as f64) / 256.0).ceil() as i32;
            let min_ty = (tl_py / 256.0).floor() as i32;
            let max_ty = ((tl_py + rect.height() as f64) / 256.0).ceil() as i32;

            // Spawn downloads for missing visible tiles
            let max_tile_coord = (1u32 << tile_state.zoom) as i32;
            for tx in min_tx..=max_tx {
                for ty in min_ty..=max_ty {
                    if tx < 0 || ty < 0 || tx >= max_tile_coord || ty >= max_tile_coord {
                        continue;
                    }
                    let key = TileKey {
                        z: tile_state.zoom,
                        x: tx as u32,
                        y: ty as u32,
                    };
                    if !tile_state.tiles.contains_key(&key) {
                        tile_state.tiles.insert(key, TileState::Loading);
                        let provider = tile_state.provider.clone();
                        let task = bevy::tasks::IoTaskPool::get().spawn(async move {
                            download_tile(key, provider)
                        });
                        commands.spawn(TileDownloadTask { task, key, _provider: tile_state.provider });
                    }
                }
            }

            // ── Draw tiles ──
            let painter = ui.painter();

            let visible_tiles = (max_tx - min_tx + 1) * (max_ty - min_ty + 1);
            let ready_count = tile_state.tiles.values().filter(|s| matches!(s, TileState::Ready(_, _))).count();
            let loading_count = tile_state.tiles.values().filter(|s| matches!(s, TileState::Loading)).count();
            let failed_count = tile_state.tiles.values().filter(|s| matches!(s, TileState::Failed)).count();
            println!("Minimap render: zoom={}, visible={}x{}={} tiles | ready={}, loading={}, failed={}",
                tile_state.zoom, max_tx - min_tx + 1, max_ty - min_ty + 1, visible_tiles,
                ready_count, loading_count, failed_count);

            let bg_color = match tile_state.provider {
                MapProvider::CartoDark => egui::Color32::from_rgb(20, 25, 30),
                MapProvider::EsriSatellite => egui::Color32::from_rgb(20, 25, 35),
                _ => egui::Color32::from_rgb(200, 210, 220),
            };
            painter.rect_filled(rect, 0.0, bg_color);

            let provider = tile_state.provider;
            for tx in min_tx..=max_tx {
                for ty in min_ty..=max_ty {
                    if tx < 0 || ty < 0 || tx >= max_tile_coord || ty >= max_tile_coord {
                        continue;
                    }
                    let key = TileKey {
                        z: tile_state.zoom,
                        x: tx as u32,
                        y: ty as u32,
                    };
                    let tile_px = tx as f64 * 256.0;
                    let tile_py = ty as f64 * 256.0;
                    let screen_x = tile_px - tl_px + rect.min.x as f64;
                    let screen_y = tile_py - tl_py + rect.min.y as f64;
                    let tile_rect = egui::Rect::from_min_size(
                        egui::pos2(screen_x as f32, screen_y as f32),
                        egui::Vec2::new(256.0, 256.0),
                    );

                    match tile_state.tiles.get_mut(&key) {
                        Some(TileState::Ready(color_image, opt_handle)) => {
                            let texture_id = if let Some(handle) = opt_handle.as_ref() {
                                handle.id()
                            } else {
                                let handle = ui.ctx().load_texture(
                                    format!("tile_{}_{}_{}_{:?}", key.z, key.x, key.y, provider),
                                    color_image.clone(),
                                    egui::TextureOptions::default(),
                                );
                                let id = handle.id();
                                *opt_handle = Some(handle);
                                id
                            };
                            let uv = egui::Rect::from_min_max(
                                egui::pos2(0.0, 0.0),
                                egui::pos2(1.0, 1.0),
                            );
                            painter.image(texture_id, tile_rect, uv, egui::Color32::WHITE);
                        }
                        Some(TileState::Loading) => {
                            let loading_color = match provider {
                                MapProvider::CartoDark => egui::Color32::from_gray(40),
                                _ => egui::Color32::from_gray(220),
                            };
                            painter.rect_filled(tile_rect, 0.0, loading_color);
                            // Loading dots animation
                            let center = tile_rect.center();
                            let time = ui.input(|i| i.time);
                            for i in 0..3 {
                                let offset = (time * 3.0 + i as f64 * 1.2).sin() as f32 * 3.0;
                                let dot_pos = egui::pos2(
                                    center.x + (i as f32 - 1.0) * 8.0,
                                    center.y + offset,
                                );
                                painter.circle_filled(
                                    dot_pos,
                                    2.5,
                                    egui::Color32::from_gray(150),
                                );
                            }
                        }
                        Some(TileState::Failed) | None => {
                            let failed_color = match provider {
                                MapProvider::CartoDark => egui::Color32::from_gray(30),
                                _ => egui::Color32::from_gray(200),
                            };
                            painter.rect_filled(tile_rect, 0.0, failed_color);
                            painter.rect_stroke(
                                tile_rect.shrink(1.0),
                                0.0,
                                egui::Stroke::new(1.0, egui::Color32::from_gray(180)),
                            );
                            // Small cross to indicate failed tile
                            let center = tile_rect.center();
                            painter.line_segment(
                                [center - egui::Vec2::new(4.0, 4.0), center + egui::Vec2::new(4.0, 4.0)],
                                egui::Stroke::new(1.0, egui::Color32::from_gray(160)),
                            );
                            painter.line_segment(
                                [center + egui::Vec2::new(-4.0, 4.0), center + egui::Vec2::new(4.0, -4.0)],
                                egui::Stroke::new(1.0, egui::Color32::from_gray(160)),
                            );
                        }
                    }
                }
            }

            // ── Draw lat/lon grid ──
            if tile_state.show_grid {
                draw_lat_lon_grid(&painter, rect, &*tile_state, tile_state.zoom);
            }

            // ── Draw drones on top ──
            let mut clicked_drone: Option<crate::core::types::DroneId> = None;
            let click_pos = response.interact_pointer_pos();

            for (_entity, identity, kinematics, _mission_state, _flight_control) in drone_query.iter() {
                let gps = geo.world_to_gps(kinematics.position);
                let (px, py) = lat_lon_to_pixel(gps.latitude, gps.longitude, tile_state.zoom);
                let screen_x = px - tl_px + rect.min.x as f64;
                let screen_y = py - tl_py + rect.min.y as f64;

                if screen_x < rect.min.x as f64
                    || screen_x > rect.max.x as f64
                    || screen_y < rect.min.y as f64
                    || screen_y > rect.max.y as f64
                {
                    continue;
                }

                let pos = egui::pos2(screen_x as f32, screen_y as f32);
                let is_selected = input_state.selected_drone == Some(identity.id);
                let dot_radius = if is_selected { 8.0 } else { 5.0 };

                painter.circle_filled(pos, dot_radius, egui::Color32::RED);
                painter.circle_stroke(pos, dot_radius, egui::Stroke::new(1.5, egui::Color32::WHITE));

                if is_selected {
                    painter.circle_stroke(pos, dot_radius + 3.0, egui::Stroke::new(2.0, egui::Color32::YELLOW));
                }

                let forward = kinematics.orientation * Vec3::Z;
                let dir = egui::Vec2::new(forward.x, -forward.z);
                let dir = if dir.length_sq() > 0.0001 {
                    dir.normalized() * 12.0
                } else {
                    egui::Vec2::new(0.0, 12.0)
                };
                painter.line_segment([pos, pos + dir], egui::Stroke::new(2.0, egui::Color32::YELLOW));

                if let Some(mouse) = click_pos {
                    let dist_sq = (mouse - pos).length_sq();
                    if dist_sq < (dot_radius + 5.0).powi(2) {
                        clicked_drone = Some(identity.id);
                    }
                }

                // Drone label
                if tile_state.show_labels {
                    let id_str = format!("{:?}", identity.id.0);
                    painter.text(
                        pos + egui::Vec2::new(0.0, -12.0),
                        egui::Align2::CENTER_BOTTOM,
                        &format!("Drone {}", &id_str[..id_str.len().min(6)]),
                        egui::FontId::proportional(10.0),
                        egui::Color32::WHITE,
                    );
                }
            }

            if response.clicked() {
                if let Some(drone_id) = clicked_drone {
                    input_state.selected_drone = Some(drone_id);
                } else if let Some(mouse) = click_pos {
                    if let Some(selected_id) = input_state.selected_drone {
                        let mouse_px = tl_px + (mouse.x - rect.min.x) as f64;
                        let mouse_py = tl_py + (mouse.y - rect.min.y) as f64;
                        let (lat, lon) = pixel_to_lat_lon(mouse_px, mouse_py, tile_state.zoom);

                        for (_entity, identity, _kinematics, mut mission_state, mut flight_control) in drone_query.iter_mut() {
                            if identity.id == selected_id {
                                let alt = 20.0f64;
                                let wp = crate::core::types::Waypoint {
                                    latitude: lat,
                                    longitude: lon,
                                    altitude_agl: alt,
                                    hold_time_secs: 0.0,
                                };
                                mission_state.mission = Some(crate::core::types::Mission {
                                    name: format!("Map Target {:.6},{:.6}", lat, lon),
                                    waypoints: vec![wp],
                                    loop_mission: false,
                                });
                                mission_state.current_waypoint = 0;
                                flight_control.mode = crate::core::types::FlightMode::Auto;
                                break;
                            }
                        }
                    }
                }
            }

            if response.secondary_clicked() {
                if let Some(mouse) = click_pos {
                    let mouse_px = tl_px + (mouse.x - rect.min.x) as f64;
                    let mouse_py = tl_py + (mouse.y - rect.min.y) as f64;
                    let (lat, lon) = pixel_to_lat_lon(mouse_px, mouse_py, tile_state.zoom);
                    navigate_events.send(ReloadWorldEvent { lat, lon });
                }
            }

            if response.double_clicked() && clicked_drone.is_none() {
                if let Some(mouse) = click_pos {
                    let mouse_px = tl_px + (mouse.x - rect.min.x) as f64;
                    let mouse_py = tl_py + (mouse.y - rect.min.y) as f64;
                    let (lat, lon) = pixel_to_lat_lon(mouse_px, mouse_py, tile_state.zoom);
                    spawn_events.send(SpawnDroneEvent {
                        lat,
                        lon,
                        drone_type: ui_prefs.selected_spawn_type,
                    });
                }
            }

            // ── Overlay UI ──
            painter.rect_stroke(rect, 0.0, egui::Stroke::new(2.0, egui::Color32::from_rgb(60, 65, 70)));

            // Compass rose in top-left
            let compass_center = rect.left_top() + egui::Vec2::new(25.0, 25.0);
            painter.circle_stroke(compass_center, 15.0, egui::Stroke::new(1.5, egui::Color32::from_rgba_unmultiplied(255, 255, 255, 180)));
            // N arrow
            let n_tip = compass_center + egui::Vec2::new(0.0, -12.0);
            painter.line_segment([compass_center, n_tip], egui::Stroke::new(2.0, egui::Color32::from_rgba_unmultiplied(255, 100, 100, 200)));
            painter.text(
                n_tip - egui::Vec2::new(0.0, 2.0),
                egui::Align2::CENTER_BOTTOM,
                "N",
                egui::FontId::proportional(10.0),
                egui::Color32::from_rgba_unmultiplied(255, 100, 100, 200),
            );

            // Mouse position lat/lon
            if let Some(mouse_pos) = response.hover_pos() {
                let mouse_px = tl_px + (mouse_pos.x - rect.min.x) as f64;
                let mouse_py = tl_py + (mouse_pos.y - rect.min.y) as f64;
                let (mouse_lat, mouse_lon) = pixel_to_lat_lon(mouse_px, mouse_py, tile_state.zoom);
                painter.text(
                    rect.right_bottom() + egui::Vec2::new(-10.0, -10.0),
                    egui::Align2::RIGHT_BOTTOM,
                    &format!("{:.5}°, {:.5}°", mouse_lat, mouse_lon),
                    egui::FontId::proportional(10.0),
                    egui::Color32::from_rgba_unmultiplied(200, 200, 200, 180),
                );
            }

            painter.text(
                rect.left_bottom() + egui::Vec2::new(10.0, -10.0),
                egui::Align2::LEFT_BOTTOM,
                &format!("Zoom: {} | Drag to pan | Scroll to zoom | Right-click to navigate", tile_state.zoom),
                egui::FontId::proportional(11.0),
                egui::Color32::LIGHT_GRAY,
            );
        });
}
