pub mod physics;
pub mod camera;
pub mod controller;
pub mod visual;
pub mod types;

use std::collections::{HashMap, HashSet};
use bevy::prelude::*;
use bevy::render::{RenderApp, Render, RenderSet};
use crossbeam_channel::bounded;
use crate::core::config::DroneConfig;
use crate::core::gps::GeoReference;
use crate::core::types::{DroneId, DroneType, FlightMode, GpsCoord, Mission, Pose, SensorSnapshot, SimTimestamp};
use crate::eval::trace::TraceCollector;
use crate::world::terrain::TerrainData;
use types::DroneTypeRegistry;

/// Phase-based system sets for ordering drone simulation systems.
///
/// Ordering: Input → FlightControl → Physics → (Collision | Camera | Visual)
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum DroneSystemSet {
    /// Keyboard/mouse input processing
    Input,
    /// Flight mode logic and control surface updates
    FlightControl,
    /// Physics integration: forces, velocities, positions
    Physics,
    /// Terrain and building collision detection, damage effects
    Collision,
    /// Per-drone camera transform updates
    Camera,
    /// Visual frame capture from drone cameras
    Visual,
}

/// Plugin for drone simulation
pub struct DronePlugin;

impl Plugin for DronePlugin {
    fn build(&self, app: &mut App) {
        let (job_sender, job_receiver) = bounded(64);
        let (result_sender, result_receiver) = bounded(64);

        app.insert_resource(visual::CaptureJobSender { sender: job_sender })
            .insert_resource(visual::CaptureResultReceiver { receiver: result_receiver })
            .init_resource::<visual::VisualFrameBuffer>()
            .init_resource::<DroneTypeRegistry>()
            .add_systems(Startup, spawn_drones.after(crate::world::terrain::insert_terrain_data))
            .add_systems(Startup, select_first_drone.after(spawn_drones))
            .configure_sets(Update, (
                DroneSystemSet::Input,
                DroneSystemSet::FlightControl.after(DroneSystemSet::Input),
                DroneSystemSet::Physics.after(DroneSystemSet::FlightControl),
                DroneSystemSet::Collision.after(DroneSystemSet::Physics),
                DroneSystemSet::Camera.after(DroneSystemSet::Physics),
                DroneSystemSet::Visual.after(DroneSystemSet::Physics),
            ))
            .add_systems(Update, controller::process_input.in_set(DroneSystemSet::Input))
            .add_systems(Update, controller::update_flight_mode.in_set(DroneSystemSet::FlightControl))
            .add_systems(Update, physics::apply_gravity.in_set(DroneSystemSet::Physics))
            .add_systems(Update, physics::update_physics.in_set(DroneSystemSet::Physics))
            .add_systems(Update, physics::terrain_collision.in_set(DroneSystemSet::Collision))
            .add_systems(Update, physics::building_collision.in_set(DroneSystemSet::Collision))
            .add_systems(Update, physics::update_damage_effects.in_set(DroneSystemSet::Collision))
            .add_systems(Update, cleanup_destroyed_drones.in_set(DroneSystemSet::Collision))
            .add_systems(Update, camera::update_camera_views.in_set(DroneSystemSet::Camera))
            .add_systems(Update, visual::capture_visual_frames.in_set(DroneSystemSet::Visual))
            .add_systems(Update, camera::cleanup_camera_map.after(cleanup_destroyed_drones))
            .add_systems(Update, visual::cleanup_visual_buffer.after(cleanup_destroyed_drones))
            .add_systems(Update, despawn_drone_entities.after(crate::handle_reload_world))
            .add_systems(Update, clear_drone_data.after(crate::handle_reload_world))
            .add_systems(Update, respawn_drones_on_reload.after(crate::world::reload_osm_data))
            .add_event::<DroneDestroyedEvent>();

        let render_app = app.sub_app_mut(RenderApp);
        render_app
            .insert_resource(visual::CaptureJobReceiver { receiver: job_receiver })
            .insert_resource(visual::CaptureResultSender { sender: result_sender })
            .add_systems(Render, visual::process_frame_captures.in_set(RenderSet::Cleanup));
    }
}

pub fn despawn_drone_entities(
    mut commands: Commands,
    mut events: EventReader<crate::DespawnWorldEvent>,
    drone_entities: Query<Entity, With<DroneIdentity>>,
) {
    for _ in events.read() {
        for entity in drone_entities.iter() {
            commands.entity(entity).despawn_recursive();
        }
    }
}

pub fn cleanup_destroyed_drones(
    mut commands: Commands,
    query: Query<(Entity, &DroneIdentity, &Health)>,
    mut event_writer: EventWriter<DroneDestroyedEvent>,
) {
    for (entity, identity, health) in query.iter() {
        if health.damage_level == DamageLevel::Destroyed {
            event_writer.send(DroneDestroyedEvent { drone_id: identity.id });
            commands.entity(entity).despawn_recursive();
        }
    }
}

pub fn clear_drone_data(
    mut events: EventReader<crate::ClearDroneDataEvent>,
    mut camera_map: ResMut<camera::DroneCameraMap>,
    mut frame_buffer: ResMut<visual::VisualFrameBuffer>,
) {
    for _ in events.read() {
        camera_map.map.clear();
        camera_map.images.clear();
        frame_buffer.frames.clear();
        frame_buffer.last_capture.clear();
    }
}

pub fn respawn_drones_on_reload(
    mut events: EventReader<crate::RespawnDronesEvent>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    config: Res<DroneConfig>,
    registry: Res<DroneTypeRegistry>,
    geo: Res<GeoReference>,
    terrain: Res<TerrainData>,
    mut trace: ResMut<TraceCollector>,
) {
    for _ in events.read() {
        spawn_drones_impl(&mut commands, &mut meshes, &mut materials, &config, &registry, &geo, &terrain, &mut trace);
    }
}

/// Damage level of a drone
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageLevel {
    None,
    Minor,
    Major,
    Critical,
    Destroyed,
}

/// Event emitted when a drone is destroyed and should be cleaned up
#[derive(Event)]
pub struct DroneDestroyedEvent {
    pub drone_id: DroneId,
}

/// Identity + type marker - EVERY drone entity has this
#[derive(Component)]
pub struct DroneIdentity {
    pub id: DroneId,
    pub drone_type: DroneType,
}

/// Kinematics - position, orientation, velocity
#[derive(Component)]
pub struct Kinematics {
    pub position: Vec3,
    pub orientation: Quat,
    pub velocity: Vec3,
    pub angular_velocity: Vec3,
}

/// GPS coordinate (derived from position, updated each frame)
#[derive(Component)]
pub struct GpsPosition {
    pub coord: GpsCoord,
}

/// Flight control inputs and mode
#[derive(Component)]
pub struct FlightControl {
    pub mode: FlightMode,
    pub target_velocity: Vec3,
    pub target_yaw: f32,
    pub thrust: f32,
    pub angular_thrust: Vec3,
}

/// Battery state
#[derive(Component)]
pub struct Battery {
    pub current_charge_mah: f32,
    pub voltage: f32,
    pub percent: f32,
}

/// Mission/waypoint navigation
#[derive(Component)]
pub struct MissionState {
    pub mission: Option<Mission>,
    pub current_waypoint: usize,
}

/// Health and damage
#[derive(Component)]
pub struct Health {
    pub health_percent: f32,
    pub damage_level: DamageLevel,
    pub is_operational: bool,
    pub last_impact_velocity: f32,
}

/// Fleet resource for O(1) drone lookups and selection management
#[derive(Resource, Default)]
pub struct Fleet {
    /// Maps DroneId to Entity for O(1) lookups
    pub drones: HashMap<DroneId, Entity>,
    /// Currently selected drone(s)
    pub selected: HashSet<DroneId>,
}

impl Fleet {
    pub fn get_entity(&self, id: DroneId) -> Option<Entity> {
        self.drones.get(&id).copied()
    }
    pub fn select(&mut self, id: DroneId) {
        self.selected.clear();
        self.selected.insert(id);
    }
    pub fn select_all(&mut self) {
        self.selected = self.drones.keys().copied().collect();
    }
    pub fn is_selected(&self, id: DroneId) -> bool {
        self.selected.contains(&id)
    }
}

/// Spawn all drone components as a bundle on a single entity
pub fn spawn_drone_bundle(
    commands: &mut Commands,
    id: DroneId,
    drone_type: DroneType,
    position: Vec3,
    gps: GpsCoord,
    registry: &DroneTypeRegistry,
) -> Entity {
    let spec = registry.specs.get(&drone_type).expect("Unknown drone type");

    commands.spawn((
        DroneIdentity { id, drone_type },
        Kinematics {
            position,
            orientation: Quat::IDENTITY,
            velocity: Vec3::ZERO,
            angular_velocity: Vec3::ZERO,
        },
        GpsPosition { coord: gps },
        FlightControl {
            mode: FlightMode::Stabilize,
            target_velocity: Vec3::ZERO,
            target_yaw: 0.0,
            thrust: 0.0,
            angular_thrust: Vec3::ZERO,
        },
        Battery {
            current_charge_mah: spec.battery.capacity_mah,
            voltage: spec.battery.voltage,
            percent: 100.0,
        },
        MissionState {
            mission: None,
            current_waypoint: 0,
        },
        Health {
            health_percent: 100.0,
            damage_level: DamageLevel::None,
            is_operational: true,
            last_impact_velocity: 0.0,
        },
    )).id()
}

/// Create a sensor snapshot from drone components
pub fn sensor_snapshot(kinematics: &Kinematics, gps: &GpsPosition, battery: &Battery, _identity: &DroneIdentity, flight_control: &FlightControl) -> SensorSnapshot {
    SensorSnapshot {
        timestamp: SimTimestamp::now(),
        gps: gps.coord,
        pose: Pose {
            position: kinematics.position,
            orientation: kinematics.orientation,
            velocity: kinematics.velocity,
            angular_velocity: kinematics.angular_velocity,
        },
        battery_percent: battery.percent,
        flight_mode: flight_control.mode,
    }
}

pub fn spawn_drones_impl(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    config: &DroneConfig,
    registry: &DroneTypeRegistry,
    geo: &GeoReference,
    terrain: &TerrainData,
    trace: &mut TraceCollector,
) {
    let spawn_alt = config.spawn_altitude_agl;

    for i in 0..config.spawn_count {
        let drone_type = config.per_drone_types.get(i).copied()
            .unwrap_or(config.default_drone_type);
        let spec = registry.specs.get(&drone_type).unwrap();
        let airframe = &spec.airframe;

        let angle = (i as f32 / config.spawn_count.max(1) as f32) * std::f32::consts::TAU;
        let radius = 5.0;
        let x = angle.cos() * radius;
        let z = angle.sin() * radius;
        let y = terrain.get_height_at_world(x, z) + spawn_alt;

        let position = Vec3::new(x, y, z);
        let gps = geo.world_to_gps(position);
        let drone_id = DroneId::new();

        let body_color = config.user_body_color
            .map(|c| Color::srgb(c[0], c[1], c[2]))
            .unwrap_or(airframe.body_color);
        let arm_color = config.user_arm_color
            .map(|c| Color::srgb(c[0], c[1], c[2]))
            .unwrap_or(airframe.arm_color);

        let drone_entity = spawn_drone_bundle(commands, drone_id, drone_type, position, gps, registry);

        commands.entity(drone_entity).insert((
            PbrBundle {
                mesh: meshes.add(Cuboid::new(
                    airframe.body_dimensions.x,
                    airframe.body_dimensions.y,
                    airframe.body_dimensions.z,
                )),
                material: materials.add(StandardMaterial {
                    base_color: body_color,
                    metallic: 0.5,
                    perceptual_roughness: 0.4,
                    ..default()
                }),
                transform: Transform::from_translation(position),
                ..default()
            },
            Name::new(format!("{} {:?}", spec.name, drone_id)),
        ));

        for arm in 0..airframe.arm_count {
            let arm_angle = (arm as f32 / airframe.arm_count as f32) * std::f32::consts::TAU
                + std::f32::consts::FRAC_PI_4;
            let arm_x = arm_angle.cos() * airframe.arm_length;
            let arm_z = arm_angle.sin() * airframe.arm_length;
            let arm_len = airframe.arm_length * 2.0;

            commands.spawn(PbrBundle {
                mesh: meshes.add(Cuboid::new(
                    airframe.arm_thickness.x,
                    airframe.arm_thickness.y,
                    arm_len,
                )),
                material: materials.add(StandardMaterial {
                    base_color: arm_color,
                    ..default()
                }),
                transform: Transform::from_xyz(arm_x, 0.0, arm_z)
                    .with_rotation(Quat::from_rotation_y(arm_angle)),
                ..default()
            }).set_parent(drone_entity);
        }

        if drone_type == DroneType::VtolFixedWing {
            commands.spawn(PbrBundle {
                mesh: meshes.add(Cuboid::new(0.8, 0.02, 0.25)),
                material: materials.add(StandardMaterial {
                    base_color: Color::srgb(0.9, 0.9, 0.9),
                    ..default()
                }),
                transform: Transform::from_xyz(0.0, 0.05, 0.0),
                ..default()
            }).set_parent(drone_entity);
        }

        trace.record_event(crate::core::types::SimEvent::DroneSpawned {
            drone_id,
            timestamp: SimTimestamp::now(),
            initial_gps: gps,
        });
    }
}

fn spawn_drones(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    config: Res<DroneConfig>,
    registry: Res<DroneTypeRegistry>,
    geo: Res<GeoReference>,
    terrain: Res<TerrainData>,
    mut trace: ResMut<TraceCollector>,
) {
    spawn_drones_impl(&mut commands, &mut meshes, &mut materials, &config, &registry, &geo, &terrain, &mut trace);
}

fn update_fleet(
    drone_query: Query<(Entity, &DroneIdentity)>,
    mut fleet: ResMut<Fleet>,
) {
    fleet.drones.clear();
    for (entity, identity) in drone_query.iter() {
        fleet.drones.insert(identity.id, entity);
    }
}

fn select_first_drone(
    drone_query: Query<&DroneIdentity>,
    mut input_state: ResMut<controller::DroneInputState>,
    mut fleet: ResMut<Fleet>,
) {
    if input_state.selected_drone.is_none() {
        if let Some(identity) = drone_query.iter().next() {
            input_state.selected_drone = Some(identity.id);
            fleet.select(identity.id);
        }
    }
}
