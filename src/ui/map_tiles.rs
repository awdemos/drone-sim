use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use std::collections::HashMap;

use crate::core::mercator::*;
use crate::core::types::{AirspaceGeometry, AirspaceRestrictionType, UserGeofence};
use crate::faa::loader::AirspaceData;
use crate::ui::{ReloadWorldEvent, UiPreferences};
use crate::drone::{FleetRegistry, SpawnDroneEvent};

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
    pub show_trails: bool,
    pub show_waypoints: bool,
    pub show_airspace: bool,
    pub show_geofences: bool,
    pub drawing_geofence: bool,
    pub geofence_vertices: Vec<(f64, f64)>,
    pub user_geofences: Vec<UserGeofence>,
    pub dragging_drone: Option<crate::core::types::DroneId>,
    pub pending_drag_move: Option<(crate::core::types::DroneId, Vec3)>,
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
            show_trails: true,
            show_waypoints: true,
            show_airspace: false,
            show_geofences: true,
            drawing_geofence: false,
            geofence_vertices: Vec::new(),
            user_geofences: Vec::new(),
            dragging_drone: None,
            pending_drag_move: None,
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
        let screen_x = (px - tl_px + rect.min.x as f64).round();

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
        let screen_y = (py - tl_py + rect.min.y as f64).round();

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
    mut fleet_registry: ResMut<FleetRegistry>,
    mut drone_query: Query<(Entity, &crate::drone::DroneIdentity, &crate::drone::Kinematics, &mut crate::drone::MissionState, &mut crate::drone::FlightControl)>,
    trail_query: Query<&crate::drone::FlightTrail, With<crate::drone::DroneIdentity>>,
    geo: Res<crate::core::gps::GeoReference>,
    mut navigate_events: EventWriter<ReloadWorldEvent>,
    ui_prefs: Res<UiPreferences>,
    mut spawn_events: EventWriter<SpawnDroneEvent>,
    panel_vis: Res<crate::ui::PanelVisibility>,
    airspace: Res<AirspaceData>,
) {
    if !panel_vis.map {
        return;
    }
    let Some(ctx) = contexts.try_ctx_mut() else {
        return;
    };
    egui::Window::new("Map")
        .default_pos([20.0, 60.0])
        .default_size([900.0, 700.0])
        .show(ctx, |ui| {
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
                ui.checkbox(&mut tile_state.show_trails, "Trails");
                ui.checkbox(&mut tile_state.show_waypoints, "Waypoints");
                ui.checkbox(&mut tile_state.show_airspace, "Airspace");
                ui.checkbox(&mut tile_state.show_geofences, "Geofences");
            });

            ui.horizontal(|ui| {
                let drawing = tile_state.drawing_geofence;
                let btn_text = if drawing { "Stop Drawing" } else { "Draw Geofence" };
                if ui.button(btn_text).clicked() {
                    tile_state.drawing_geofence = !drawing;
                    if !tile_state.drawing_geofence {
                        if tile_state.geofence_vertices.len() >= 3 {
                            let name = format!("Geofence {}", tile_state.user_geofences.len() + 1);
                            let vertices = tile_state.geofence_vertices.clone();
                            tile_state.user_geofences.push(UserGeofence {
                                name,
                                vertices,
                                max_altitude_ft: None,
                            });
                        }
                        tile_state.geofence_vertices.clear();
                    }
                }
                if drawing {
                    ui.label(format!("{} vertices", tile_state.geofence_vertices.len()));
                }
                if !tile_state.user_geofences.is_empty() && ui.button("Clear All").clicked() {
                    tile_state.user_geofences.clear();
                }
                if ui.button("Save").clicked() {
                    let _ = save_geofences(&tile_state.user_geofences);
                }
                if ui.button("Load").clicked() {
                    if let Some(geofences) = load_geofences() {
                        tile_state.user_geofences = geofences;
                    }
                }
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
                if tile_state.dragging_drone.is_some() {
                    if let Some(mouse) = response.interact_pointer_pos() {
                        let mouse_px = tile_state.center_pixel_x + (mouse.x - rect.center().x) as f64;
                        let mouse_py = tile_state.center_pixel_y + (mouse.y - rect.center().y) as f64;
                        let (lat, lon) = pixel_to_lat_lon(mouse_px, mouse_py, tile_state.zoom);
                        let new_pos = geo.gps_to_world(&crate::core::types::GpsCoord {
                            latitude: lat,
                            longitude: lon,
                            altitude_msl: 0.0,
                        });
                        let drag_id = tile_state.dragging_drone.unwrap();
                        tile_state.pending_drag_move = Some((drag_id, new_pos));
                    }
                } else {
                    let delta = response.drag_delta();
                    tile_state.center_pixel_x -= delta.x as f64;
                    tile_state.center_pixel_y -= delta.y as f64;
                }
            }

            if response.drag_stopped() {
                tile_state.dragging_drone = None;
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
            // Round to whole pixels to prevent sub-pixel jitter
            let tl_px = (tile_state.center_pixel_x - rect.width() as f64 / 2.0).round();
            let tl_py = (tile_state.center_pixel_y - rect.height() as f64 / 2.0).round();
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
            let _ = visible_tiles;

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
                    let screen_x = (tile_px - tl_px + rect.min.x as f64).round();
                    let screen_y = (tile_py - tl_py + rect.min.y as f64).round();
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

            // ── Draw airspace restrictions ──
            if tile_state.show_airspace {
                draw_airspace_zones(&painter, rect, &*airspace, tl_px, tl_py, tile_state.zoom);
            }

            if tile_state.show_geofences {
                draw_user_geofences(&painter, rect, &*tile_state, tl_px, tl_py, tile_state.zoom);
            }

            // ── Draw drones on top ──
            let mut hover_drone: Option<crate::core::types::DroneId> = None;
            if let Some(mouse) = response.hover_pos() {
            for (_entity, identity, kinematics, _mission_state, _flight_control) in drone_query.iter() {
                    let gps = geo.world_to_gps(kinematics.position);
                    let (px, py) = lat_lon_to_pixel(gps.latitude, gps.longitude, tile_state.zoom);
                    let sx = (px - tl_px + rect.min.x as f64).round();
                    let sy = (py - tl_py + rect.min.y as f64).round();
                    let pos = egui::pos2(sx as f32, sy as f32);
                    if (mouse - pos).length_sq() < 144.0 {
                        hover_drone = Some(identity.id);
                        break;
                    }
                }
            }

            if response.drag_started() {
                if let Some(drone_id) = hover_drone {
                    tile_state.dragging_drone = Some(drone_id);
                    fleet_registry.select_single(drone_id);
                }
            }

            let mut clicked_drone: Option<crate::core::types::DroneId> = None;
            let click_pos = response.interact_pointer_pos();

            for (entity, identity, kinematics, mission_state, _flight_control) in drone_query.iter() {
                let gps = geo.world_to_gps(kinematics.position);
                let (px, py) = lat_lon_to_pixel(gps.latitude, gps.longitude, tile_state.zoom);
                let screen_x = (px - tl_px + rect.min.x as f64).round();
                let screen_y = (py - tl_py + rect.min.y as f64).round();

                if screen_x < rect.min.x as f64
                    || screen_x > rect.max.x as f64
                    || screen_y < rect.min.y as f64
                    || screen_y > rect.max.y as f64
                {
                    continue;
                }

                let pos = egui::pos2(screen_x as f32, screen_y as f32);
                let is_selected = fleet_registry.is_selected(identity.id);
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
                    painter.text(
                        pos + egui::Vec2::new(0.0, -12.0),
                        egui::Align2::CENTER_BOTTOM,
                        &identity.name,
                        egui::FontId::proportional(10.0),
                        egui::Color32::WHITE,
                    );
                }

                if tile_state.show_trails {
                    if let Ok(trail) = trail_query.get(entity) {
                        if trail.points.len() >= 2 {
                            let screen_pts: Vec<egui::Pos2> = trail.points.iter()
                                .filter_map(|p| {
                                    let tg = geo.world_to_gps(*p);
                                    let (tpx, tpy) = lat_lon_to_pixel(tg.latitude, tg.longitude, tile_state.zoom);
                                    let sx = (tpx - tl_px + rect.min.x as f64).round();
                                    let sy = (tpy - tl_py + rect.min.y as f64).round();
                                    if sx >= rect.min.x as f64 - 10.0 && sx <= rect.max.x as f64 + 10.0
                                        && sy >= rect.min.y as f64 - 10.0 && sy <= rect.max.y as f64 + 10.0
                                    {
                                        Some(egui::pos2(sx as f32, sy as f32))
                                    } else {
                                        None
                                    }
                                })
                                .collect();
                            if screen_pts.len() >= 2 {
                                let trail_color = if is_selected {
                                    egui::Color32::from_rgba_unmultiplied(255, 200, 50, 120)
                                } else {
                                    egui::Color32::from_rgba_unmultiplied(255, 80, 80, 80)
                                };
                                for pair in screen_pts.windows(2) {
                                    painter.line_segment([pair[0], pair[1]], egui::Stroke::new(1.5, trail_color));
                                }
                            }
                        }
                    }
                }

                if tile_state.show_waypoints {
                    if let Some(ref mission) = mission_state.mission {
                        if !mission.waypoints.is_empty() {
                            let wp_color = if is_selected {
                                egui::Color32::from_rgba_unmultiplied(0, 255, 150, 220)
                            } else {
                                egui::Color32::from_rgba_unmultiplied(0, 200, 120, 140)
                            };
                            let wp_active = egui::Color32::from_rgba_unmultiplied(255, 255, 0, 240);

                            let mut wp_screen_pts = Vec::new();
                            for (i, wp) in mission.waypoints.iter().enumerate() {
                                let (wpx, wpy) = lat_lon_to_pixel(wp.latitude, wp.longitude, tile_state.zoom);
                                let sx = (wpx - tl_px + rect.min.x as f64).round();
                                let sy = (wpy - tl_py + rect.min.y as f64).round();
                                if sx >= rect.min.x as f64 && sx <= rect.max.x as f64
                                    && sy >= rect.min.y as f64 && sy <= rect.max.y as f64
                                {
                                    let s = egui::pos2(sx as f32, sy as f32);
                                    wp_screen_pts.push((s, i));
                                }
                            }

                            if wp_screen_pts.len() >= 2 {
                                for pair in wp_screen_pts.windows(2) {
                                    painter.line_segment([pair[0].0, pair[1].0], egui::Stroke::new(2.0, wp_color));
                                }
                            }

                            for (s, i) in wp_screen_pts {
                                let is_current = i == mission_state.current_waypoint;
                                let color = if is_current { wp_active } else { wp_color };
                                let radius = if is_current { 8.0 } else { 5.0 };
                                painter.circle_filled(s, radius, color);
                                painter.circle_stroke(s, radius, egui::Stroke::new(1.5, egui::Color32::WHITE));
                                let label = format!("{}", i + 1);
                                painter.text(
                                    s + egui::Vec2::new(0.0, -radius - 4.0),
                                    egui::Align2::CENTER_BOTTOM,
                                    &label,
                                    egui::FontId::proportional(if is_current { 11.0 } else { 9.0 }),
                                    egui::Color32::WHITE,
                                );
                            }
                        }
                    }
                }
            }

            if response.clicked() {
                if tile_state.drawing_geofence {
                    if let Some(mouse) = click_pos {
                        let mouse_px = tl_px + (mouse.x - rect.min.x) as f64;
                        let mouse_py = tl_py + (mouse.y - rect.min.y) as f64;
                        let (lat, lon) = pixel_to_lat_lon(mouse_px, mouse_py, tile_state.zoom);
                        tile_state.geofence_vertices.push((lat, lon));
                    }
                } else if let Some(drone_id) = clicked_drone {
                    fleet_registry.select_single(drone_id);
                } else if let Some(mouse) = click_pos {
                    let selected_ids: Vec<crate::core::types::DroneId> = fleet_registry.selected().iter().copied().collect();
                    if let Some(&selected_id) = selected_ids.first() {
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
                    let alt: f64 = ui_prefs.spawn_alt.parse().unwrap_or(20.0);
                    spawn_events.send(SpawnDroneEvent {
                        lat,
                        lon,
                        alt,
                        drone_type: ui_prefs.selected_spawn_type,
                    });
                    let spawn_pos = egui::pos2(mouse.x, mouse.y);
                    painter.circle(spawn_pos, 16.0, egui::Color32::from_rgba_unmultiplied(0, 255, 100, 80), egui::Stroke::new(2.0, egui::Color32::GREEN));
                    painter.text(
                        spawn_pos + egui::Vec2::new(0.0, -20.0),
                        egui::Align2::CENTER_BOTTOM,
                        "SPAWNED",
                        egui::FontId::proportional(11.0),
                        egui::Color32::GREEN,
                    );
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
                &format!("Zoom: {} | Drag=pan | Scroll=zoom | Right-click=navigate | Dbl-click=spawn", tile_state.zoom),
                egui::FontId::proportional(11.0),
                egui::Color32::LIGHT_GRAY,
            );
        });
}

pub fn apply_map_drag(
    mut tile_state: ResMut<MapTileState>,
    mut drone_query: Query<(&crate::drone::DroneIdentity, &mut crate::drone::Kinematics)>,
) {
    if let Some((drag_id, new_pos)) = tile_state.pending_drag_move.take() {
        for (identity, mut kinematics) in drone_query.iter_mut() {
            if identity.id == drag_id {
                kinematics.position.x = new_pos.x;
                kinematics.position.z = new_pos.z;
                kinematics.velocity = bevy::prelude::Vec3::ZERO;
                break;
            }
        }
    }
}

fn draw_airspace_zones(
    painter: &egui::Painter,
    rect: egui::Rect,
    airspace: &AirspaceData,
    tl_px: f64,
    tl_py: f64,
    zoom: u32,
) {
    let center_lat = if let Some(mouse) = painter.ctx().input(|i| i.pointer.hover_pos()) {
        let mouse_px = tl_px + (mouse.x - rect.min.x) as f64;
        let mouse_py = tl_py + (mouse.y - rect.min.y) as f64;
        let (lat, _) = pixel_to_lat_lon(mouse_px, mouse_py, zoom);
        lat
    } else {
        let (_, lat) = pixel_to_lat_lon(tl_px + rect.width() as f64 / 2.0, tl_py + rect.height() as f64 / 2.0, zoom);
        lat
    };

    let lat_rad = center_lat.to_radians();
    let meters_per_px = 156543.03 * lat_rad.cos() / (1u32 << zoom) as f64;

    for zone in airspace.all_zones() {
        let (fill_color, stroke_color) = match zone.zone_type {
            AirspaceRestrictionType::NoFly => (
                egui::Color32::from_rgba_unmultiplied(255, 0, 0, 30),
                egui::Color32::from_rgba_unmultiplied(255, 0, 0, 150),
            ),
            AirspaceRestrictionType::HeightRestricted => (
                egui::Color32::from_rgba_unmultiplied(255, 200, 0, 25),
                egui::Color32::from_rgba_unmultiplied(255, 200, 0, 120),
            ),
            AirspaceRestrictionType::Warning => (
                egui::Color32::from_rgba_unmultiplied(255, 140, 0, 25),
                egui::Color32::from_rgba_unmultiplied(255, 140, 0, 120),
            ),
        };

        match &zone.geometry {
            AirspaceGeometry::Circle { center_lat, center_lon, radius_meters } => {
                let (px, py) = lat_lon_to_pixel(*center_lat, *center_lon, zoom);
                let screen_x = (px - tl_px + rect.min.x as f64) as f32;
                let screen_y = (py - tl_py + rect.min.y as f64) as f32;

                if screen_x < rect.min.x - 200.0
                    || screen_x > rect.max.x + 200.0
                    || screen_y < rect.min.y - 200.0
                    || screen_y > rect.max.y + 200.0
                {
                    continue;
                }

                let radius_px = (*radius_meters / meters_per_px) as f32;
                let center = egui::pos2(screen_x, screen_y);

                painter.circle_filled(center, radius_px, fill_color);
                painter.circle_stroke(center, radius_px, egui::Stroke::new(2.0, stroke_color));

                if let Some(max_ft) = zone.max_altitude_ft {
                    painter.text(
                        center + egui::Vec2::new(0.0, -radius_px - 6.0),
                        egui::Align2::CENTER_BOTTOM,
                        &format!("{:.0}ft", max_ft),
                        egui::FontId::proportional(10.0),
                        stroke_color,
                    );
                }

                painter.text(
                    center + egui::Vec2::new(0.0, radius_px + 4.0),
                    egui::Align2::CENTER_TOP,
                    &zone.name,
                    egui::FontId::proportional(9.0),
                    stroke_color,
                );
            }
            AirspaceGeometry::Polygon { vertices } => {
                let screen_points: Vec<egui::Pos2> = vertices
                    .iter()
                    .filter_map(|(lat, lon)| {
                        let (px, py) = lat_lon_to_pixel(*lat, *lon, zoom);
                        let sx = (px - tl_px + rect.min.x as f64) as f32;
                        let sy = (py - tl_py + rect.min.y as f64) as f32;
                        if sx >= rect.min.x - 50.0
                            && sx <= rect.max.x + 50.0
                            && sy >= rect.min.y - 50.0
                            && sy <= rect.max.y + 50.0
                        {
                            Some(egui::pos2(sx, sy))
                        } else {
                            None
                        }
                    })
                    .collect();

                if screen_points.len() >= 3 {
                    painter.add(egui::Shape::convex_polygon(
                        screen_points.clone(),
                        fill_color,
                        egui::Stroke::new(1.5, stroke_color),
                    ));

                    if let Some(first) = screen_points.first() {
                        painter.text(
                            *first + egui::Vec2::new(0.0, -12.0),
                            egui::Align2::CENTER_BOTTOM,
                            &zone.name,
                            egui::FontId::proportional(9.0),
                            stroke_color,
                        );
                    }
                }
            }
        }
    }
}

fn save_geofences(geofences: &[UserGeofence]) -> Result<(), String> {
    let path = std::path::Path::new("data/geofences.json");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let json = serde_json::to_string_pretty(geofences).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}

fn load_geofences() -> Option<Vec<UserGeofence>> {
    let path = std::path::Path::new("data/geofences.json");
    if !path.exists() {
        return None;
    }
    let json = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&json).ok()
}

fn draw_user_geofences(
    painter: &egui::Painter,
    rect: egui::Rect,
    tile_state: &MapTileState,
    tl_px: f64,
    tl_py: f64,
    zoom: u32,
) {
    let fill_color = egui::Color32::from_rgba_unmultiplied(255, 140, 0, 40);
    let stroke_color = egui::Color32::from_rgba_unmultiplied(255, 140, 0, 180);
    let vertex_color = egui::Color32::from_rgba_unmultiplied(255, 200, 50, 220);

        for geofence in &tile_state.user_geofences {
            let screen_points: Vec<egui::Pos2> = geofence.vertices
                .iter()
                .filter_map(|(lat, lon)| {
                let (px, py) = lat_lon_to_pixel(*lat, *lon, zoom);
                let sx = (px - tl_px + rect.min.x as f64) as f32;
                let sy = (py - tl_py + rect.min.y as f64) as f32;
                if sx >= rect.min.x - 50.0 && sx <= rect.max.x + 50.0
                    && sy >= rect.min.y - 50.0 && sy <= rect.max.y + 50.0
                {
                    Some(egui::pos2(sx, sy))
                } else {
                    None
                }
            })
            .collect();

        if screen_points.len() >= 3 {
            painter.add(egui::Shape::convex_polygon(
                screen_points.clone(),
                fill_color,
                egui::Stroke::new(2.0, stroke_color),
            ));
            for pt in &screen_points {
                painter.circle_filled(*pt, 4.0, vertex_color);
            }
            if let Some(first) = screen_points.first() {
                painter.text(
                    *first + egui::Vec2::new(0.0, -14.0),
                    egui::Align2::CENTER_BOTTOM,
                    &geofence.name,
                    egui::FontId::proportional(10.0),
                    stroke_color,
                );
            }
        }
    }

    if tile_state.drawing_geofence && !tile_state.geofence_vertices.is_empty() {
        let screen_points: Vec<egui::Pos2> = tile_state.geofence_vertices
            .iter()
            .filter_map(|(lat, lon)| {
                let (px, py) = lat_lon_to_pixel(*lat, *lon, zoom);
                let sx = (px - tl_px + rect.min.x as f64) as f32;
                let sy = (py - tl_py + rect.min.y as f64) as f32;
                if sx >= rect.min.x - 50.0 && sx <= rect.max.x + 50.0
                    && sy >= rect.min.y - 50.0 && sy <= rect.max.y + 50.0
                {
                    Some(egui::pos2(sx, sy))
                } else {
                    None
                }
            })
            .collect();

        for pt in &screen_points {
            painter.circle_filled(*pt, 4.0, vertex_color);
        }
        if screen_points.len() >= 2 {
            for pair in screen_points.windows(2) {
                painter.line_segment([pair[0], pair[1]], egui::Stroke::new(2.0, stroke_color));
            }
        }
        if screen_points.len() >= 3 {
            painter.add(egui::Shape::convex_polygon(
                screen_points.clone(),
                fill_color,
                egui::Stroke::new(2.0, stroke_color),
            ));
        }
    }
}
