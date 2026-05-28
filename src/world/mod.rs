pub mod terrain;
pub mod osm_loader;
pub mod buildings;
pub mod satellite_terrain;
pub mod earth_backdrop;

use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

pub struct WorldPlugin;

/// Marker component for world entities that should be despawned on navigation
#[derive(Component)]
pub struct WorldEntity;

/// Axis-aligned bounding box for building collision
#[derive(Component, Clone)]
pub struct BuildingCollider {
    pub min: Vec3,
    pub max: Vec3,
}

/// 3D spatial hash grid for O(1) building collision lookups.
#[derive(Resource)]
pub struct SpatialGrid {
    cell_size: f32,
    cells: HashMap<(i32, i32, i32), Vec<(Entity, BuildingCollider)>>,
}

impl SpatialGrid {
    pub fn new(cell_size: f32) -> Self {
        Self {
            cell_size,
            cells: HashMap::new(),
        }
    }

    /// Insert a building collider into all overlapping grid cells.
    pub fn insert(&mut self, entity: Entity, collider: BuildingCollider) {
        let cell_size = self.cell_size;
        let min_cell = (
            (collider.min.x / cell_size).floor() as i32,
            (collider.min.y / cell_size).floor() as i32,
            (collider.min.z / cell_size).floor() as i32,
        );
        let max_cell = (
            (collider.max.x / cell_size).floor() as i32,
            (collider.max.y / cell_size).floor() as i32,
            (collider.max.z / cell_size).floor() as i32,
        );

        for x in min_cell.0..=max_cell.0 {
            for y in min_cell.1..=max_cell.1 {
                for z in min_cell.2..=max_cell.2 {
                    self.cells
                        .entry((x, y, z))
                        .or_default()
                        .push((entity, collider.clone()));
                }
            }
        }
    }

    /// Remove all entries from the grid.
    pub fn clear(&mut self) {
        self.cells.clear();
    }

    /// Query all building colliders whose cells overlap the sphere at `position` with `radius`.
    /// Returns deduplicated (Entity, &BuildingCollider) pairs.
    pub fn query_near(&self, position: Vec3, radius: f32) -> Vec<(Entity, &BuildingCollider)> {
        let cell_size = self.cell_size;
        let min_cell = (
            ((position.x - radius) / cell_size).floor() as i32,
            ((position.y - radius) / cell_size).floor() as i32,
            ((position.z - radius) / cell_size).floor() as i32,
        );
        let max_cell = (
            ((position.x + radius) / cell_size).floor() as i32,
            ((position.y + radius) / cell_size).floor() as i32,
            ((position.z + radius) / cell_size).floor() as i32,
        );

        let mut seen = HashSet::new();
        let mut results = Vec::new();
        for x in min_cell.0..=max_cell.0 {
            for y in min_cell.1..=max_cell.1 {
                for z in min_cell.2..=max_cell.2 {
                    if let Some(entries) = self.cells.get(&(x, y, z)) {
                        for (entity, collider) in entries {
                            if seen.insert(*entity) {
                                results.push((*entity, collider));
                            }
                        }
                    }
                }
            }
        }
        results
    }
}

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(SpatialGrid::new(50.0))
            .insert_resource(DayNightCycle::default())
            .add_plugins(satellite_terrain::SatelliteTerrainPlugin)
            .add_plugins(earth_backdrop::EarthBackdropPlugin)
            .add_systems(Startup, setup_lighting)
            .add_systems(Startup, setup_environment.after(setup_lighting))
            .add_systems(Update, update_day_night)
            .add_systems(Update, despawn_world_entities.after(crate::handle_reload_world))
            .add_systems(Update, reload_osm_data.after(despawn_world_entities));
    }
}

pub fn despawn_world_entities(
    mut commands: Commands,
    mut events: EventReader<crate::events::DespawnWorldEvent>,
    world_entities: Query<Entity, With<WorldEntity>>,
    mut spatial_grid: ResMut<SpatialGrid>,
) {
    for _ in events.read() {
        spatial_grid.clear();
        for entity in world_entities.iter() {
            commands.entity(entity).despawn_recursive();
        }
    }
}

pub fn reload_osm_data(
    mut events: EventReader<crate::events::ReloadOsmEvent>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut world_config: ResMut<crate::core::config::WorldConfig>,
    mut terrain_state: ResMut<satellite_terrain::SatelliteTerrainState>,
    mut spatial_grid: ResMut<SpatialGrid>,
) {
    for event in events.read() {
        world_config.origin_lat = event.lat;
        world_config.origin_lon = event.lon;
        let geo = crate::core::gps::GeoReference::new(event.lat, event.lon, 0.0);
        commands.insert_resource(geo.clone());

        let osm_data = crate::load_osm_with_fallback(world_config.osm_path.as_deref(), &geo);
        commands.insert_resource(osm_data.clone());

        println!("Navigated to: {:.6}, {:.6}", event.lat, event.lon);
        println!("Buildings: {}", osm_data.buildings.len());
        println!("Roads: {}", osm_data.roads.len());

        let terrain = terrain::TerrainData::generate(&world_config);
        commands.insert_resource(terrain.clone());

        buildings::spawn_buildings_and_roads_impl(&mut commands, &mut meshes, &mut materials, &osm_data, &mut spatial_grid);

        satellite_terrain::start_satellite_tile_downloads(&mut commands, &mut terrain_state, &world_config);
    }
}

#[derive(Component)]
pub struct SunLight;

fn setup_lighting(mut commands: Commands) {
    commands.spawn((
        DirectionalLightBundle {
            directional_light: DirectionalLight {
                illuminance: 100_000.0,
                shadows_enabled: true,
                shadow_depth_bias: 0.05,
                shadow_normal_bias: 0.5,
                ..default()
            },
            transform: Transform::from_xyz(100.0, 200.0, 100.0).looking_at(Vec3::ZERO, Vec3::Y),
            ..default()
        },
        SunLight,
        WorldEntity,
    ));

    commands.insert_resource(AmbientLight {
        color: Color::srgb(0.8, 0.9, 1.0),
        brightness: 0.4,
    });
}

#[derive(Resource)]
pub struct DayNightCycle {
    pub enabled: bool,
    pub time_speed: f32,
    pub latitude_deg: f32,
}

impl Default for DayNightCycle {
    fn default() -> Self {
        Self {
            enabled: true,
            time_speed: 1.0,
            latitude_deg: 37.77,
        }
    }
}

fn update_day_night(
    mut sun_transform: Query<&mut Transform, With<SunLight>>,
    mut sun_light: Query<&mut DirectionalLight, With<SunLight>>,
    mut ambient: ResMut<AmbientLight>,
    mut clear_color: ResMut<ClearColor>,
    day_night: Res<DayNightCycle>,
    world_config: Res<crate::core::config::WorldConfig>,
) {
    if !day_night.enabled {
        return;
    }

    let now = chrono::Utc::now();
    let hour_utc = (now.timestamp() % 86400) as f32 / 3600.0;
    let lat = world_config.origin_lat as f32;

    let solar_angle = (hour_utc / 24.0 * std::f32::consts::TAU) - std::f32::consts::PI;
    let elevation = solar_angle.sin() * lat.to_radians().cos();
    let azimuth = solar_angle.cos();

    let sun_distance = 300.0;
    let sun_y = elevation * sun_distance;
    let sun_x = azimuth * sun_distance;

    if let Ok(mut transform) = sun_transform.get_single_mut() {
        *transform = Transform::from_xyz(sun_x, sun_y.max(5.0), 50.0)
            .looking_at(Vec3::ZERO, Vec3::Y);
    }

    let day_factor = ((elevation * 3.0).clamp(-1.0, 1.0) * 0.5 + 0.5).clamp(0.0, 1.0);

    if let Ok(mut light) = sun_light.get_single_mut() {
        light.illuminance = 1_000.0 + day_factor * 99_000.0;
    }

    ambient.brightness = 0.05 + day_factor * 0.45;
    ambient.color = Color::srgb(
        0.3 + day_factor * 0.5,
        0.3 + day_factor * 0.6,
        0.4 + day_factor * 0.5,
    );

    let sky_r = 0.05 + day_factor * 0.30;
    let sky_g = 0.05 + day_factor * 0.50;
    let sky_b = 0.15 + day_factor * 0.65;
    clear_color.0 = Color::srgb(sky_r, sky_g, sky_b);
}

fn setup_environment(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(ClearColor(Color::srgb(0.35, 0.55, 0.8)));

    let water_mesh = meshes.add(Plane3d::default().mesh().size(10000.0, 10000.0));
    commands.spawn((
        PbrBundle {
            mesh: water_mesh,
            material: materials.add(StandardMaterial {
                base_color: Color::srgba(0.1, 0.3, 0.5, 0.7),
                alpha_mode: AlphaMode::Blend,
                perceptual_roughness: 0.1,
                metallic: 0.1,
                ..default()
            }),
            transform: Transform::from_xyz(0.0, -2.0, 0.0),
            ..default()
        },
        WorldEntity,
        Name::new("Water Plane"),
    ));
}
