use bevy::prelude::*;
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use image::RgbaImage;
use std::collections::HashMap;

use crate::core::config::WorldConfig;
use crate::core::gps::GeoReference;
use crate::core::mercator::{lat_lon_to_pixel, TileKey};
use crate::world::WorldEntity;

pub struct SatelliteTerrainPlugin;

impl Plugin for SatelliteTerrainPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SatelliteTerrainState>()
            .init_resource::<TerrainTileRetryQueue>()
            .add_systems(Startup, init_satellite_terrain)
            .add_systems(Update, poll_satellite_tile_downloads)
            .add_systems(Update, process_terrain_tile_retries)
            .add_systems(Update, reset_terrain_state.after(crate::handle_reload_world));
    }
}

pub fn reset_terrain_state(
    mut events: EventReader<crate::events::DespawnWorldEvent>,
    mut terrain_state: ResMut<SatelliteTerrainState>,
) {
    for _ in events.read() {
        terrain_state.tiles_needed.clear();
        terrain_state.tiles_ready.clear();
        terrain_state.terrain_spawned = false;
    }
}

/// Tile state for the 3D satellite terrain ground plane.
///
/// Separate from `MapTileState` (UI minimap) because terrain uses a fixed
/// ESRI provider, builds a texture atlas, and spawns a Bevy mesh entity.
#[derive(Resource, Default)]
pub struct SatelliteTerrainState {
    pub tiles_needed: Vec<TileKey>,
    pub tiles_ready: HashMap<TileKey, image::RgbaImage>,
    pub terrain_spawned: bool,
    pub zoom: u32,
    pub atlas_tiles_x: u32,
    pub atlas_tiles_y: u32,
    pub min_tx: u32,
    pub min_ty: u32,
    pub generation: u32,
}

#[derive(Component)]
pub struct SatelliteTileTask {
    pub task: bevy::tasks::Task<anyhow::Result<Vec<u8>>>,
    pub key: TileKey,
    pub generation: u32,
}

#[derive(Resource, Default)]
pub struct TerrainTileRetryQueue {
    pub retries: Vec<(TileKey, f32)>,
}

fn compute_tile_coverage(
    origin_lat: f64,
    origin_lon: f64,
    terrain_size_m: f32,
) -> (u32, u32, u32, u32, u32) {
    let lat_rad = origin_lat.to_radians();
    let cos_lat = lat_rad.cos();
    let earth_circumference = 40_075_016.686_f64;

    let mut best_zoom = 16u32;
    let mut best_tile_count = 0f64;

    for z in 14..=19 {
        let tile_size_m = earth_circumference * cos_lat / ((1u32 << z) as f64);
        let tiles_across = terrain_size_m as f64 / tile_size_m;
        let score = (tiles_across - 12.0).abs();
        if best_tile_count == 0.0 || score < (best_tile_count - 12.0).abs() {
            best_tile_count = tiles_across;
            best_zoom = z;
        }
    }

    let zoom = best_zoom;
    let half_size = terrain_size_m / 2.0;
    let geo = GeoReference::new(origin_lat, origin_lon, 0.0);

    let corners = [
        (half_size, half_size),
        (half_size, -half_size),
        (-half_size, half_size),
        (-half_size, -half_size),
    ];

    let mut min_px = f64::INFINITY;
    let mut max_px = f64::NEG_INFINITY;
    let mut min_py = f64::INFINITY;
    let mut max_py = f64::NEG_INFINITY;

    for &(dx, dz) in &corners {
        let enu = crate::core::types::EnuCoord {
            east: dx as f64,
            north: dz as f64,
            up: 0.0,
        };
        let gps = geo.enu_to_gps(&enu);
        let (px, py) = lat_lon_to_pixel(gps.latitude, gps.longitude, zoom);
        min_px = min_px.min(px);
        max_px = max_px.max(px);
        min_py = min_py.min(py);
        max_py = max_py.max(py);
    }

    let min_tx = (min_px / 256.0).floor() as u32;
    let max_tx = (max_px / 256.0).ceil() as u32;
    let min_ty = (min_py / 256.0).floor() as u32;
    let max_ty = (max_py / 256.0).ceil() as u32;

    let tiles_x = max_tx - min_tx + 1;
    let tiles_y = max_ty - min_ty + 1;

    println!(
        "Satellite terrain: zoom={}, tiles={}x{} ({} total)",
        zoom,
        tiles_x,
        tiles_y,
        tiles_x * tiles_y,
    );

    (zoom, min_tx, max_tx, min_ty, max_ty)
}

fn download_tile_blocking(key: TileKey) -> anyhow::Result<Vec<u8>> {
    let cache_path = std::path::PathBuf::from(format!(
        "data/tiles/{}/{}/{}.jpg",
        key.z, key.x, key.y
    ));
    if cache_path.exists() {
        return Ok(std::fs::read(&cache_path)?);
    }

    let url = format!(
        "https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{}/{}/{}",
        key.z, key.y, key.x
    );
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;
    let response = client
        .get(&url)
        .header("User-Agent", "drone-sim/1.0")
        .send()?;
    anyhow::ensure!(response.status().is_success(), "HTTP {}", response.status());
    let bytes = response.bytes()?.to_vec();

    if let Some(parent) = cache_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&cache_path, &bytes);

    Ok(bytes)
}

pub fn start_satellite_tile_downloads(
    commands: &mut Commands,
    state: &mut SatelliteTerrainState,
    config: &WorldConfig,
) {
    let (zoom, min_tx, max_tx, min_ty, max_ty) = compute_tile_coverage(
        config.origin_lat,
        config.origin_lon,
        config.terrain_size_m,
    );

    state.zoom = zoom;
    state.min_tx = min_tx;
    state.min_ty = min_ty;
    state.atlas_tiles_x = max_tx - min_tx + 1;
    state.atlas_tiles_y = max_ty - min_ty + 1;
    state.terrain_spawned = false;
    state.tiles_needed.clear();
    state.tiles_ready.clear();
    state.generation = state.generation.wrapping_add(1);
    let generation = state.generation;

    for tx in min_tx..=max_tx {
        for ty in min_ty..=max_ty {
            let key = TileKey { z: zoom, x: tx, y: ty };
            state.tiles_needed.push(key);

            let task = bevy::tasks::IoTaskPool::get().spawn(async move {
                download_tile_blocking(key)
            });
            commands.spawn(SatelliteTileTask { task, key, generation });
        }
    }

    println!(
        "Downloading {} satellite tiles for 3D terrain...",
        state.tiles_needed.len()
    );
}

fn init_satellite_terrain(
    mut commands: Commands,
    mut state: ResMut<SatelliteTerrainState>,
    config: Res<WorldConfig>,
) {
    start_satellite_tile_downloads(&mut commands, &mut state, &config);
}

fn poll_satellite_tile_downloads(
    mut commands: Commands,
    mut tasks: Query<(Entity, &mut SatelliteTileTask)>,
    mut state: ResMut<SatelliteTerrainState>,
    mut retry_queue: ResMut<TerrainTileRetryQueue>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    config: Res<WorldConfig>,
    geo: Res<GeoReference>,
) {
    let current_generation = state.generation;
    let mut success_count = 0usize;
    let mut fail_count = 0usize;

    for (entity, mut task) in tasks.iter_mut() {
        if task.generation != current_generation {
            commands.entity(entity).despawn();
            continue;
        }
        if let Some(result) =
            bevy::tasks::block_on(bevy::tasks::futures_lite::future::poll_once(&mut task.task))
        {
            match result {
                Ok(bytes) => {
                    match image::load_from_memory(&bytes) {
                        Ok(img) => {
                            let rgba = img.to_rgba8();
                            state.tiles_ready.insert(task.key, rgba);
                            success_count += 1;
                        }
                        Err(e) => {
                            eprintln!("Satellite tile decode error for {:?}: {}", task.key, e);
                            retry_queue.retries.push((task.key, 3.0));
                            fail_count += 1;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Satellite tile download error for {:?}: {}", task.key, e);
                    retry_queue.retries.push((task.key, 3.0));
                    fail_count += 1;
                }
            }
            commands.entity(entity).despawn();
        }
    }

    if success_count > 0 || fail_count > 0 {
        println!(
            "Terrain tiles: +{} success, +{} fail, {}/{} total ready",
            success_count, fail_count, state.tiles_ready.len(), state.tiles_needed.len()
        );
    }

    if state.terrain_spawned {
        return;
    }

    let total_needed = state.tiles_needed.len();
    let total_ready = state.tiles_ready.len();

    if total_needed > 0 && total_ready < total_needed / 2 {
        return;
    }

    let mut real_tiles = 0usize;
    let mut placeholder_tiles = 0usize;
    for key in &state.tiles_needed {
        if state.tiles_ready.contains_key(key) {
            real_tiles += 1;
        } else {
            placeholder_tiles += 1;
        }
    }

    println!(
        "Building terrain atlas with {}/{} satellite tiles ({:.0}%) — {} real, {} placeholder",
        state.tiles_ready.len(),
        state.tiles_needed.len(),
        100.0 * total_ready as f32 / total_needed as f32,
        real_tiles,
        placeholder_tiles,
    );

    let atlas_width = state.atlas_tiles_x * 256;
    let atlas_height = state.atlas_tiles_y * 256;

    let mut atlas = RgbaImage::new(atlas_width, atlas_height);

    for key in &state.tiles_needed {
        let dest_x = (key.x - state.min_tx) * 256;
        let dest_y = (key.y - state.min_ty) * 256;
        if let Some(tile_img) = state.tiles_ready.get(key) {
            for y in 0..256u32 {
                for x in 0..256u32 {
                    let pixel = tile_img.get_pixel(x, y);
                    atlas.put_pixel(dest_x + x, dest_y + y, *pixel);
                }
            }
        } else {
            for y in 0..256u32 {
                for x in 0..256u32 {
                    atlas.put_pixel(dest_x + x, dest_y + y, image::Rgba([64, 128, 64, 255]));
                }
            }
        }
    }

    let mut bevy_image = Image::new(
        Extent3d {
            width: atlas_width,
            height: atlas_height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        atlas.into_raw(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    bevy_image.sampler = bevy::render::texture::ImageSampler::linear();
    let texture_handle = images.add(bevy_image);

    spawn_textured_terrain(
        &mut commands,
        &mut meshes,
        &mut materials,
        &config,
        &geo,
        state.zoom,
        state.min_tx,
        state.min_ty,
        state.atlas_tiles_x,
        state.atlas_tiles_y,
        texture_handle,
    );

    state.terrain_spawned = true;
    println!("Satellite terrain spawned with {}x{} pixel atlas", atlas_width, atlas_height);
}

fn spawn_textured_terrain(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    config: &WorldConfig,
    geo: &GeoReference,
    zoom: u32,
    min_tx: u32,
    min_ty: u32,
    atlas_tiles_x: u32,
    atlas_tiles_y: u32,
    texture_handle: Handle<Image>,
) {
    let res = config.terrain_resolution as usize;
    let size = config.terrain_size_m;
    let half_size = size / 2.0;

    println!("=== TERRAIN DIAGNOSTICS ===");
    println!("Terrain size: {}m, resolution: {}, half_size: {}", size, res, half_size);
    println!("Atlas tiles: {}x{} ({} total)", atlas_tiles_x, atlas_tiles_y, atlas_tiles_x * atlas_tiles_y);
    println!("Zoom level: {}", zoom);
    println!("Tile range: x=[{}, {}], y=[{}, {}]", min_tx, min_tx + atlas_tiles_x - 1, min_ty, min_ty + atlas_tiles_y - 1);

    let mut positions = Vec::with_capacity(res * res);
    let mut uvs = Vec::with_capacity(res * res);
    let mut indices = Vec::with_capacity((res - 1) * (res - 1) * 6);

    let mut heights = vec![vec![0.0f32; res]; res];
    for z in 0..res {
        for x in 0..res {
            let nx = x as f32 / (res - 1).max(1) as f32;
            let nz = z as f32 / (res - 1).max(1) as f32;
            let mut h = 0.0f32;
            let mut amp = 10.0f32;
            let mut freq = 1.0f32;
            for _ in 0..4 {
                h += simple_noise(nx * freq, nz * freq) * amp;
                amp *= 0.5;
                freq *= 2.0;
            }
            let dx = nx - 0.5;
            let dz = nz - 0.5;
            let dist_from_center = (dx * dx + dz * dz).sqrt();
            let flatten_factor = (dist_from_center * 4.0).min(1.0);
            h *= flatten_factor;
            heights[z][x] = h.max(0.0);
        }
    }

    let atlas_pixel_width = (atlas_tiles_x * 256) as f64;
    let atlas_pixel_height = (atlas_tiles_y * 256) as f64;
    let min_px = min_tx as f64 * 256.0;
    let min_py = min_ty as f64 * 256.0;

    println!("Atlas pixel bounds: x=[{}, {}], y=[{}, {}]", min_px, min_px + atlas_pixel_width, min_py, min_py + atlas_pixel_height);

    let mut uv_min = [f32::INFINITY, f32::INFINITY];
    let mut uv_max = [f32::NEG_INFINITY, f32::NEG_INFINITY];
    let mut uv_out_of_bounds = 0usize;
    let mut nan_uv_count = 0usize;
    let mut world_min = [f32::INFINITY, f32::INFINITY, f32::INFINITY];
    let mut world_max = [f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];

    for z in 0..res {
        for x in 0..res {
            let px = (x as f32 / (res - 1).max(1) as f32) * size - half_size;
            let pz = (z as f32 / (res - 1).max(1) as f32) * size - half_size;
            let py = heights[z][x];
            positions.push([px, py, pz]);

            world_min[0] = world_min[0].min(px);
            world_min[1] = world_min[1].min(py);
            world_min[2] = world_min[2].min(pz);
            world_max[0] = world_max[0].max(px);
            world_max[1] = world_max[1].max(py);
            world_max[2] = world_max[2].max(pz);

            let gps = geo.world_to_gps(Vec3::new(px, 0.0, pz));
            let (pixel_x, pixel_y) = lat_lon_to_pixel(gps.latitude, gps.longitude, zoom);

            let u = ((pixel_x - min_px) / atlas_pixel_width) as f32;
            let v = 1.0 - ((pixel_y - min_py) / atlas_pixel_height) as f32;

            uv_min[0] = uv_min[0].min(u);
            uv_min[1] = uv_min[1].min(v);
            uv_max[0] = uv_max[0].max(u);
            uv_max[1] = uv_max[1].max(v);

            if u.is_nan() || v.is_nan() {
                nan_uv_count += 1;
                println!("  NaN UV at grid({},{}): world=({:.1},{:.1}), gps=({:.6},{:.6}), pixel=({:.1},{:.1})",
                    x, z, px, pz, gps.latitude, gps.longitude, pixel_x, pixel_y);
            } else if u < -0.01 || u > 1.01 || v < -0.01 || v > 1.01 {
                uv_out_of_bounds += 1;
                if uv_out_of_bounds <= 5 {
                    println!("  OOB UV at grid({},{}): u={:.3}, v={:.3}, world=({:.1},{:.1}), gps=({:.6},{:.6}), pixel=({:.1},{:.1})",
                        x, z, u, v, px, pz, gps.latitude, gps.longitude, pixel_x, pixel_y);
                }
            }

            uvs.push([u.clamp(0.0, 1.0), v.clamp(0.0, 1.0)]);
        }
    }

    println!("World bounds: min=({:.1},{:.1},{:.1}), max=({:.1},{:.1},{:.1})",
        world_min[0], world_min[1], world_min[2],
        world_max[0], world_max[1], world_max[2]);
    println!("UV range: u=[{:.3},{:.3}], v=[{:.3},{:.3}]",
        uv_min[0], uv_max[0], uv_min[1], uv_max[1]);
    println!("UV out of bounds: {} ({} NaN)", uv_out_of_bounds, nan_uv_count);

    let corners = [
        (0, 0, "NW"),
        (res - 1, 0, "NE"),
        (0, res - 1, "SW"),
        (res - 1, res - 1, "SE"),
    ];
    for (cx, cz, name) in corners {
        let px = (cx as f32 / (res - 1).max(1) as f32) * size - half_size;
        let pz = (cz as f32 / (res - 1).max(1) as f32) * size - half_size;
        let gps = geo.world_to_gps(Vec3::new(px, 0.0, pz));
        let (pix_x, pix_y) = lat_lon_to_pixel(gps.latitude, gps.longitude, zoom);
        let u = ((pix_x - min_px) / atlas_pixel_width) as f32;
        let v = 1.0 - ((pix_y - min_py) / atlas_pixel_height) as f32;
        println!("  Corner {}: world=({:.1},{:.1}), gps=({:.6},{:.6}), pixel=({:.1},{:.1}), uv=({:.3},{:.3})",
            name, px, pz, gps.latitude, gps.longitude, pix_x, pix_y, u, v);
    }

    for z in 0..res - 1 {
        for x in 0..res - 1 {
            let i = z * res + x;
            indices.push(i as u32);
            indices.push((i + res) as u32);
            indices.push((i + 1) as u32);
            indices.push((i + 1) as u32);
            indices.push((i + res) as u32);
            indices.push((i + res + 1) as u32);
        }
    }

    let mut mesh = Mesh::new(
        bevy::render::mesh::PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(bevy::render::mesh::Indices::U32(indices));
    mesh.compute_normals();

    commands.spawn((
        PbrBundle {
            mesh: meshes.add(mesh),
            material: materials.add(StandardMaterial {
                base_color_texture: Some(texture_handle),
                unlit: true,
                double_sided: true,
                ..default()
            }),
            ..default()
        },
        WorldEntity,
    ));

    commands.insert_resource(crate::world::terrain::TerrainData {
        heights,
        size_m: size,
        resolution: res,
    });

    println!("=== END TERRAIN DIAGNOSTICS ===");
}

fn simple_noise(x: f32, y: f32) -> f32 {
    let n = x.sin() * 43758.5453 + y.cos() * 23421.675;
    (n.fract() - 0.5) * 2.0
}

fn process_terrain_tile_retries(
    mut commands: Commands,
    mut retry_queue: ResMut<TerrainTileRetryQueue>,
    state: ResMut<SatelliteTerrainState>,
    time: Res<Time>,
) {
    let dt = time.delta_seconds();
    let mut still_pending = Vec::new();

    for (key, mut delay) in retry_queue.retries.drain(..) {
        delay -= dt;
        if delay <= 0.0 {
            let tile_still_needed = state.tiles_needed.contains(&key) && !state.tiles_ready.contains_key(&key);
            if tile_still_needed {
                let task = bevy::tasks::IoTaskPool::get().spawn(async move {
                    download_tile_blocking(key)
                });
                commands.spawn(SatelliteTileTask {
                    task,
                    key,
                    generation: state.generation,
                });
            }
        } else {
            still_pending.push((key, delay));
        }
    }

    retry_queue.retries = still_pending;
}
