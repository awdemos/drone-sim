pub mod physics;
pub mod camera;
pub mod controller;
pub mod visual;
pub mod types;

use std::collections::{HashMap, HashSet};
use bevy::prelude::*;
use bevy::render::{RenderApp, Render, RenderSet};
use crossbeam_channel::bounded;
use bevy_egui::EguiSet;
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
    /// Sync Kinematics to Transform
    TransformSync,
    /// Per-drone camera transform updates
    Camera,
    /// Visual frame capture from drone cameras
    Visual,
}

/// Plugin for drone simulation
pub struct DronePlugin;

impl Plugin for DronePlugin {
    fn build(&self, app: &mut App) {
        let env = app.world().get_resource::<crate::core::config::PhysicsConfig>()
            .map(|c| EnvironmentSettings::from(c))
            .unwrap_or_default();
        let (job_sender, job_receiver) = bounded(64);
        let (result_sender, result_receiver) = bounded(64);

        app.insert_resource(visual::CaptureJobSender { sender: job_sender })
            .insert_resource(visual::CaptureResultReceiver { receiver: result_receiver })
            .init_resource::<visual::VisualFrameBuffer>()
            .init_resource::<DroneTypeRegistry>()
            .init_resource::<FleetRegistry>()
            .insert_resource(env)
            .add_systems(Startup, spawn_drones.after(crate::world::terrain::insert_terrain_data))
            .add_systems(Startup, select_first_drone.after(spawn_drones))
            .configure_sets(Update, (
                DroneSystemSet::Input,
                DroneSystemSet::FlightControl.after(DroneSystemSet::Input),
                DroneSystemSet::Physics.after(DroneSystemSet::FlightControl),
                DroneSystemSet::Collision.after(DroneSystemSet::Physics),
                DroneSystemSet::TransformSync.after(DroneSystemSet::Physics),
                DroneSystemSet::Camera.after(DroneSystemSet::TransformSync),
                DroneSystemSet::Visual.after(DroneSystemSet::Physics),
            ))
            .add_systems(Update, update_fleet_registry.after(crate::world::terrain::insert_terrain_data))
            .add_systems(Update, controller::process_input.in_set(DroneSystemSet::Input).after(EguiSet::InitContexts))
            .add_systems(Update, controller::update_flight_mode.in_set(DroneSystemSet::FlightControl))
            .add_systems(Update, physics::apply_gravity.in_set(DroneSystemSet::Physics))
            .add_systems(Update, physics::update_physics.in_set(DroneSystemSet::Physics).after(physics::apply_gravity))
            .add_systems(Update, physics::terrain_collision.in_set(DroneSystemSet::Collision))
            .add_systems(Update, physics::building_collision.in_set(DroneSystemSet::Collision))
            .add_systems(Update, physics::update_damage_effects.in_set(DroneSystemSet::Collision))
            .add_systems(Update, cleanup_destroyed_drones.in_set(DroneSystemSet::Collision))
            .add_systems(Update, sync_drone_transforms.in_set(DroneSystemSet::TransformSync))
            .add_systems(Update, update_selection_rings.after(sync_drone_transforms))
            .add_systems(Update, drone_picking.after(sync_drone_transforms))
            .add_systems(Update, camera::update_camera_views.in_set(DroneSystemSet::Camera))
            .add_systems(Update, visual::capture_visual_frames.in_set(DroneSystemSet::Visual))
            .add_systems(Update, camera::cleanup_camera_map.after(cleanup_destroyed_drones))
            .add_systems(Update, visual::cleanup_visual_buffer.after(cleanup_destroyed_drones))
            .add_systems(Update, despawn_drone_entities.after(crate::handle_reload_world))
            .add_systems(Update, clear_drone_data.after(crate::handle_reload_world))
            .add_systems(Update, respawn_drones_on_reload.after(crate::world::reload_osm_data))
            .add_event::<DroneDestroyedEvent>()
            .add_event::<SpawnDroneEvent>()
            .add_event::<RenameDroneEvent>()
            .add_event::<ChangeSerialEvent>()
            .add_systems(Update, apply_rename_events)
            .add_systems(Update, apply_serial_events)
            .add_systems(Update, update_flight_trails.in_set(DroneSystemSet::Physics))
            .add_systems(Update, handle_spawn_drone_events.after(crate::world::terrain::insert_terrain_data));

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

/// Event to request renaming a drone
#[derive(Event)]
pub struct RenameDroneEvent {
    pub drone_id: DroneId,
    pub new_name: String,
}

/// Process rename events and apply them to drone identities
pub fn apply_rename_events(
    mut events: EventReader<RenameDroneEvent>,
    mut query: Query<&mut DroneIdentity>,
) {
    for event in events.read() {
        for mut identity in query.iter_mut() {
            if identity.id == event.drone_id {
                identity.name.clone_from(&event.new_name);
                break;
            }
        }
    }
}

/// Event to change a drone's serial number
#[derive(Event)]
pub struct ChangeSerialEvent {
    pub drone_id: DroneId,
    pub serial: Option<String>,
}

pub fn apply_serial_events(
    mut events: EventReader<ChangeSerialEvent>,
    mut query: Query<&mut DroneIdentity>,
) {
    for event in events.read() {
        for mut identity in query.iter_mut() {
            if identity.id == event.drone_id {
                identity.serial = event.serial.clone();
                break;
            }
        }
    }
}

#[derive(Event)]
pub struct SpawnDroneEvent {
    pub lat: f64,
    pub lon: f64,
    pub alt: f64,
    pub drone_type: DroneType,
}

/// Runtime-mutable environment that physics reads each frame.
/// Initialized from PhysicsConfig, editable via UI sliders.
#[derive(Resource)]
pub struct EnvironmentSettings {
    pub wind_speed_ms: f32,
    pub wind_direction_deg: f32,
    pub turbulence: f32,
    pub sea_level_density: f32,
    pub density_scale_height_m: f32,
    pub sea_level_offset_m: f32,
}

impl From<&crate::core::config::PhysicsConfig> for EnvironmentSettings {
    fn from(c: &crate::core::config::PhysicsConfig) -> Self {
        Self {
            wind_speed_ms: c.wind_speed_ms,
            wind_direction_deg: c.wind_direction_deg,
            turbulence: c.turbulence,
            sea_level_density: c.sea_level_density,
            density_scale_height_m: c.density_scale_height_m,
            sea_level_offset_m: c.sea_level_offset_m,
        }
    }
}

impl Default for EnvironmentSettings {
    fn default() -> Self {
        Self::from(&crate::core::config::PhysicsConfig::default())
    }
}

impl EnvironmentSettings {
    pub fn wind_vector(&self) -> Vec3 {
        let angle = self.wind_direction_deg.to_radians();
        Vec3::new(angle.sin() * self.wind_speed_ms, 0.0, angle.cos() * self.wind_speed_ms)
    }

    pub fn wind_with_turbulence(&self) -> Vec3 {
        let base = self.wind_vector();
        if self.turbulence <= 0.0 || self.wind_speed_ms <= 0.0 {
            return base;
        }
        let t = self.turbulence * self.wind_speed_ms;
        base + Vec3::new(
            (fastrand::f32() - 0.5) * t,
            (fastrand::f32() - 0.5) * t * 0.1,
            (fastrand::f32() - 0.5) * t,
        )
    }

    pub fn air_density_ratio(&self, altitude_m: f32) -> f32 {
        let h = (altitude_m + self.sea_level_offset_m).max(0.0);
        (-h / self.density_scale_height_m).exp()
    }
}

/// Unique identifier for a fleet of drones
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FleetId(pub u32);

/// A named collection of drones
#[derive(Debug, Clone)]
pub struct NamedFleet {
    #[allow(dead_code)]
    pub id: FleetId,
    pub name: String,
    pub drones: HashMap<DroneId, Entity>,
    pub selected: HashSet<DroneId>,
}

/// Registry of all fleets. Replaces the old single `Fleet` resource.
#[derive(Resource)]
pub struct FleetRegistry {
    pub fleets: HashMap<FleetId, NamedFleet>,
    pub active_fleet: FleetId,
    next_id: u32,
}

impl Default for FleetRegistry {
    fn default() -> Self {
        let default_fleet = NamedFleet {
            id: FleetId(0),
            name: "Default Fleet".into(),
            drones: HashMap::new(),
            selected: HashSet::new(),
        };
        let mut fleets = HashMap::new();
        fleets.insert(FleetId(0), default_fleet);
        Self {
            fleets,
            active_fleet: FleetId(0),
            next_id: 1,
        }
    }
}

impl FleetRegistry {
    /// Create a new named fleet and return its ID
    pub fn create_fleet(&mut self, name: String) -> FleetId {
        let id = FleetId(self.next_id);
        self.next_id += 1;
        self.fleets.insert(
            id,
            NamedFleet {
                id,
                name,
                drones: HashMap::new(),
                selected: HashSet::new(),
            },
        );
        id
    }

    /// Remove a fleet (cannot remove the last fleet)
    pub fn remove_fleet(&mut self, id: FleetId) {
        if self.fleets.len() <= 1 {
            return;
        }
        self.fleets.remove(&id);
        if self.active_fleet == id {
            self.active_fleet = *self.fleets.keys().next().unwrap_or(&FleetId(0));
        }
    }

    /// Rename a fleet
    pub fn rename_fleet(&mut self, id: FleetId, name: String) {
        if let Some(fleet) = self.fleets.get_mut(&id) {
            fleet.name = name;
        }
    }

    /// Switch active fleet
    pub fn set_active(&mut self, id: FleetId) {
        if self.fleets.contains_key(&id) {
            self.active_fleet = id;
        }
    }

    /// Reference to the active fleet
    pub fn active(&self) -> &NamedFleet {
        self.fleets.get(&self.active_fleet).expect("Active fleet always exists")
    }

    /// Mutable reference to the active fleet
    pub fn active_mut(&mut self) -> &mut NamedFleet {
        self.fleets.get_mut(&self.active_fleet).expect("Active fleet always exists")
    }

    /// Convenience: get the active fleet's drone map
    #[allow(dead_code)]
    pub fn drones(&self) -> &HashMap<DroneId, Entity> {
        &self.active().drones
    }

    /// Convenience: get the active fleet's selection set
    pub fn selected(&self) -> &HashSet<DroneId> {
        &self.active().selected
    }

    /// Check if a drone is selected in the active fleet
    pub fn is_selected(&self, id: DroneId) -> bool {
        self.active().selected.contains(&id)
    }

    /// Select a single drone in the active fleet (clears other selections)
    pub fn select_single(&mut self, id: DroneId) {
        let active = self.active_mut();
        active.selected.clear();
        active.selected.insert(id);
    }

    /// Select all drones in the active fleet
    pub fn select_all(&mut self) {
        let active = self.active_mut();
        active.selected = active.drones.keys().copied().collect();
    }

    /// Add a drone to a specific fleet
    pub fn add_drone(&mut self, fleet_id: FleetId, drone_id: DroneId, entity: Entity) {
        if let Some(fleet) = self.fleets.get_mut(&fleet_id) {
            fleet.drones.insert(drone_id, entity);
        }
    }

    /// Remove a drone from ALL fleets
    #[allow(dead_code)]
    pub fn remove_drone_from_all(&mut self, drone_id: DroneId) {
        for fleet in self.fleets.values_mut() {
            fleet.drones.remove(&drone_id);
            fleet.selected.remove(&drone_id);
        }
    }

    /// Lookup entity for a drone ID in the active fleet
    #[allow(dead_code)]
    pub fn get_entity(&self, drone_id: DroneId) -> Option<Entity> {
        self.active().drones.get(&drone_id).copied()
    }
}

/// Identity + type marker - EVERY drone entity has this
#[derive(Component)]
pub struct DroneIdentity {
    pub id: DroneId,
    pub drone_type: DroneType,
    pub name: String,
    pub serial: Option<String>,
    pub fleet_id: FleetId,
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

#[derive(Component)]
pub struct FlightTrail {
    pub points: Vec<Vec3>,
    pub max_points: usize,
    pub sample_timer: Timer,
}

impl Default for FlightTrail {
    fn default() -> Self {
        Self {
            points: Vec::new(),
            max_points: 500,
            sample_timer: Timer::from_seconds(0.1, TimerMode::Repeating),
        }
    }
}

fn update_flight_trails(
    time: Res<Time>,
    mut query: Query<(&Kinematics, &mut FlightTrail)>,
) {
    for (kinematics, mut trail) in query.iter_mut() {
        trail.sample_timer.tick(time.delta());
        if trail.sample_timer.just_finished() {
            trail.points.push(kinematics.position);
            if trail.points.len() > trail.max_points {
                trail.points.remove(0);
            }
        }
    }
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

#[derive(Component)]
pub struct SelectionRing;



/// Spawn all drone components as a bundle on a single entity
pub fn spawn_drone_bundle(
    commands: &mut Commands,
    id: DroneId,
    drone_type: DroneType,
    position: Vec3,
    gps: GpsCoord,
    registry: &DroneTypeRegistry,
    name: String,
    serial: Option<String>,
    fleet_id: FleetId,
) -> Entity {
    let spec = registry.specs.get(&drone_type).expect("Unknown drone type");

    commands.spawn((
        DroneIdentity { id, drone_type, name, serial, fleet_id },
        Kinematics {
            position,
            orientation: Quat::IDENTITY,
            velocity: Vec3::ZERO,
            angular_velocity: Vec3::ZERO,
        },
        GpsPosition { coord: gps },
        FlightControl {
            mode: FlightMode::AltHold,
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
        FlightTrail::default(),
    )).id()
}

pub fn spawn_visual_assets(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    drone_entity: Entity,
    spec: &types::DroneTypeSpec,
    body_color: Color,
    arm_color: Color,
    position: Vec3,
    name_label: String,
    drone_type: DroneType,
) {
    let airframe = &spec.airframe;
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
        Name::new(name_label),
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

        let id_str = format!("{:?}", drone_id.0);
        let drone_name = format!("Drone-{}", &id_str[..id_str.len().min(8)]);
        let drone_entity = spawn_drone_bundle(commands, drone_id, drone_type, position, gps, registry, drone_name.clone(), None, FleetId(0));

        let name_label = format!("{} {}", spec.name, drone_name);
        spawn_visual_assets(commands, meshes, materials, drone_entity, spec, body_color, arm_color, position, name_label, drone_type);

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

fn update_fleet_registry(
    drone_query: Query<(Entity, &DroneIdentity)>,
    mut registry: ResMut<FleetRegistry>,
) {
    for fleet in registry.fleets.values_mut() {
        fleet.drones.clear();
    }
    for (entity, identity) in drone_query.iter() {
        registry
            .fleets
            .entry(identity.fleet_id)
            .or_insert_with(|| NamedFleet {
                id: identity.fleet_id,
                name: format!("Fleet {}", identity.fleet_id.0),
                drones: HashMap::new(),
                selected: HashSet::new(),
            })
            .drones
            .insert(identity.id, entity);
    }
}

fn select_first_drone(
    drone_query: Query<&DroneIdentity>,
    mut registry: ResMut<FleetRegistry>,
) {
    if registry.selected().is_empty() {
        if let Some(identity) = drone_query.iter().next() {
            registry.select_single(identity.id);
        }
    }
}

pub fn sync_drone_transforms(
    mut drone_query: Query<(&Kinematics, &mut Transform)>,
) {
    for (kinematics, mut transform) in drone_query.iter_mut() {
        transform.translation = kinematics.position;
        transform.rotation = kinematics.orientation;
    }
}

fn update_selection_rings(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    fleet_registry: Res<FleetRegistry>,
    drone_query: Query<(Entity, &DroneIdentity, &Children)>,
    ring_query: Query<Entity, With<SelectionRing>>,
) {
    let selected_ids: HashSet<DroneId> = fleet_registry.selected().iter().copied().collect();

    for ring_entity in ring_query.iter() {
        commands.entity(ring_entity).despawn_recursive();
    }

    for (entity, identity, _children) in drone_query.iter() {
        if selected_ids.contains(&identity.id) {
            let ring_entity = commands.spawn((
                PbrBundle {
                    mesh: meshes.add(Torus::new(0.4, 0.05)),
                    material: materials.add(StandardMaterial {
                        base_color: Color::srgb(1.0, 0.9, 0.0),
                        emissive: LinearRgba::new(1.0, 0.9, 0.0, 1.0),
                        ..default()
                    }),
                    transform: Transform::from_translation(Vec3::new(0.0, -0.1, 0.0)),
                    ..default()
                },
                SelectionRing,
            )).id();
            commands.entity(entity).add_child(ring_entity);
        }
    }
}

fn drone_picking(
    mouse_button: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera_query: Query<(&Camera, &GlobalTransform), With<crate::OrbitCamera>>,
    drone_query: Query<(Entity, &DroneIdentity, &Kinematics)>,
    mut fleet_registry: ResMut<FleetRegistry>,
    mut egui_ctx: bevy_egui::EguiContexts,
) {
    if !mouse_button.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(ctx) = egui_ctx.try_ctx_mut() else {
        return;
    };
    if ctx.is_pointer_over_area() || ctx.is_using_pointer() {
        return;
    }
    let window = windows.single();
    let Some(cursor_pos) = window.cursor_position() else {
        return;
    };
    let (camera, camera_transform) = camera_query.single();
    let Some(ray) = camera.viewport_to_world(camera_transform, cursor_pos) else {
        return;
    };
    let ray_origin = ray.origin;
    let ray_dir = ray.direction.into();

    let mut closest_hit: Option<(f32, DroneId)> = None;
    let pick_radius = 0.5f32;

    for (_entity, identity, kinematics) in drone_query.iter() {
        let drone_pos = kinematics.position;
        let to_center = drone_pos - ray_origin;
        let t = to_center.dot(ray_dir);
        if t < 0.0 {
            continue;
        }
        let closest_point = ray_origin + ray_dir * t;
        let dist_sq = (closest_point - drone_pos).length_squared();
        if dist_sq < pick_radius * pick_radius {
            if closest_hit.is_none() || t < closest_hit.unwrap().0 {
                closest_hit = Some((t, identity.id));
            }
        }
    }

    if let Some((_, drone_id)) = closest_hit {
        fleet_registry.select_single(drone_id);
    }
}

pub fn handle_spawn_drone_events(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut events: EventReader<SpawnDroneEvent>,
    registry: Res<DroneTypeRegistry>,
    geo: Res<crate::core::gps::GeoReference>,
    _terrain: Res<crate::world::terrain::TerrainData>,
    mut trace: ResMut<TraceCollector>,
    mut fleet_registry: ResMut<FleetRegistry>,
) {
    for event in events.read() {
        let drone_type = event.drone_type;
        let spec = registry.specs.get(&drone_type).unwrap();
        let airframe = &spec.airframe;

        let gps = GpsCoord {
            latitude: event.lat,
            longitude: event.lon,
            altitude_msl: event.alt,
        };
        let position = geo.gps_to_world(&gps);
        let drone_id = DroneId::new();

        let body_color = airframe.body_color;
        let arm_color = airframe.arm_color;

        let id_str = format!("{:?}", drone_id.0);
        let drone_name = format!("Drone-{}", &id_str[..id_str.len().min(8)]);
        let fleet_id = fleet_registry.active_fleet;
        let drone_entity = spawn_drone_bundle(
            &mut commands, drone_id, drone_type, position, gps, &registry,
            drone_name.clone(), None, fleet_id,
        );

        let name_label = format!("{} {}", spec.name, drone_name);
        spawn_visual_assets(
            &mut commands, &mut meshes, &mut materials, drone_entity, spec,
            body_color, arm_color, position, name_label, drone_type,
        );

        trace.record_event(crate::core::types::SimEvent::DroneSpawned {
            drone_id,
            timestamp: SimTimestamp::now(),
            initial_gps: gps,
        });

        fleet_registry.select_single(drone_id);
    }
}
