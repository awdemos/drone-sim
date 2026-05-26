mod core;
mod world;
mod drone;
mod llm;
mod eval;
mod ui;
mod splash;

use bevy::prelude::*;
use bevy::window::{WindowMode, WindowResolution};
use clap::Parser;
use core::config::{load_config, save_default_config};
use core::gps::GeoReference;
use core::types::CameraMode;
use world::osm_loader::load_osm_data;

pub fn load_osm_with_fallback(path: Option<&std::path::Path>, geo: &GeoReference) -> world::osm_loader::OsmData {
    match path {
        Some(p) => load_osm_data(p, geo).unwrap_or_else(|e| {
            eprintln!("Warning: Failed to load OSM data: {}. Using procedural city.", e);
            load_osm_data(std::path::Path::new(""), geo)
                .expect("procedural city generation should never fail")
        }),
        None => load_osm_data(std::path::Path::new(""), geo)
            .expect("procedural city generation should never fail"),
    }
}
use world::terrain::insert_terrain_data;
use world::buildings::spawn_buildings_and_roads;
use drone::DronePlugin;
use drone::camera::DroneCameraMap;
use drone::controller::DroneInputState;
use drone::visual::VisualFrameBuffer;
use ui::ReloadWorldEvent;
use llm::LlmPlugin;
use llm::reasoning::ReasoningTrace;
use eval::EvalPlugin;
use ui::UiPlugin;

#[derive(Parser, Debug)]
#[command(name = "drone-sim")]
#[command(about = "3D Drone Flight Simulator with LLM Integration")]
struct Args {
    /// Path to configuration file
    #[arg(short, long)]
    config: Option<std::path::PathBuf>,
    
    /// Generate default config and exit
    #[arg(long)]
    gen_config: bool,
    
    /// Origin latitude
    #[arg(long)]
    lat: Option<f64>,
    
    /// Origin longitude  
    #[arg(long)]
    lon: Option<f64>,
    
    /// Number of drones
    #[arg(short, long, default_value = "1")]
    drones: usize,
    
    /// Enable LLM integration
    #[arg(long)]
    llm: bool,
    
    /// LLM provider (ollama, openai, moonshot, kimi)
    #[arg(long)]
    provider: Option<String>,
}

#[derive(Event)]
pub struct DespawnWorldEvent;

#[derive(Event)]
pub struct ReloadOsmEvent {
    pub lat: f64,
    pub lon: f64,
}

#[derive(Event)]
pub struct RespawnDronesEvent;

#[derive(Event)]
pub struct ClearDroneDataEvent;

fn main() {
    let _ = dotenvy::dotenv();
    let args = Args::parse();

    if args.gen_config {
        save_default_config(std::path::Path::new("config/sim.toml"))
            .expect("Failed to write default config");
        println!("Default config written to config/sim.toml");
        return;
    }

    let mut config = load_config(args.config.as_deref())
        .expect("Failed to load config");

    // Override config with CLI args
    if let Some(lat) = args.lat {
        config.world.origin_lat = lat;
    }
    if let Some(lon) = args.lon {
        config.world.origin_lon = lon;
    }
    config.drone.spawn_count = args.drones;
    if args.llm {
        config.llm.enabled = true;
    }
    if let Some(provider) = args.provider {
        config.llm.provider = provider;
    }

    // Resolve LLM API URL and key from environment for supported providers
    let resolved_url = config.llm.resolve_api_url();
    let resolved_key = config.llm.resolve_api_key();
    if config.llm.enabled {
        println!("LLM Provider: {}", config.llm.provider);
        println!("LLM API URL: {}", resolved_url);
        println!("LLM API Key: {}", if resolved_key.is_some() { "***" } else { "NOT SET" });
        if resolved_key.is_none() && matches!(config.llm.provider.as_str(), "moonshot" | "kimi" | "openai") {
            eprintln!("WARNING: No API key found for {}. Set {}_API_KEY environment variable.",
                config.llm.provider.to_uppercase(),
                config.llm.provider.to_uppercase());
        }
    }

    // Setup GeoReference
    let geo = GeoReference::new(
        config.world.origin_lat,
        config.world.origin_lon,
        0.0,
    );

    // Load OSM data
    let osm_data = load_osm_with_fallback(config.world.osm_path.as_deref(), &geo);

    println!("Starting Drone Flight Simulator");
    println!("Origin: {:.6}, {:.6}", config.world.origin_lat, config.world.origin_lon);
    println!("Drones: {}", config.drone.spawn_count);
    println!("Buildings: {}", osm_data.buildings.len());
    println!("Roads: {}", osm_data.roads.len());
    println!("LLM Enabled: {} (provider: {})", config.llm.enabled, config.llm.provider);

    App::new()
        .insert_resource(config.clone())
        .insert_resource(config.window.clone())
        .insert_resource(config.world.clone())
        .insert_resource(config.drone.clone())
        .insert_resource(config.physics.clone())
        .insert_resource(config.llm.clone())
        .insert_resource(config.eval.clone())
        .insert_resource(geo)
        .insert_resource(osm_data)
        .insert_resource(CameraMode::Overhead)
        .insert_resource(DroneCameraMap::default())
        .insert_resource(DroneInputState::default())
        .insert_resource(VisualFrameBuffer::default())
        .insert_resource(ReasoningTrace::default())
        .add_event::<DespawnWorldEvent>()
        .add_event::<ReloadOsmEvent>()
        .add_event::<RespawnDronesEvent>()
        .add_event::<ClearDroneDataEvent>()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: config.window.title.clone(),
                resolution: WindowResolution::new(
                    config.window.width as f32,
                    config.window.height as f32,
                ),
                mode: WindowMode::Windowed,
                ..default()
            }),
            ..default()
        }))
        .add_plugins(splash::SplashPlugin)
        .add_plugins(world::WorldPlugin)
        .add_plugins(DronePlugin)
        .add_plugins(LlmPlugin)
        .add_plugins(EvalPlugin)
        .add_plugins(UiPlugin)
        .add_systems(Startup, setup_scene)
        .add_systems(Startup, insert_terrain_data.after(setup_scene))
        .add_systems(Startup, spawn_buildings_and_roads.after(insert_terrain_data))
        .add_systems(Startup, sync_minimap_center.after(setup_scene))
        .add_systems(Update, camera_controller.run_if(resource_exists::<splash::AppReady>))
        .add_systems(Update, handle_reload_world)
        .add_systems(Update, debug_navigate_hotkey)
        .run();
}

#[derive(Component)]
pub struct OrbitCamera {
    pub radius: f32,
    pub theta: f32,   // horizontal angle
    pub phi: f32,     // vertical angle
    pub target: Vec3,
    pub follow_drone: bool,
    pub mode: CameraMode,
    pub eye_height: f32,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        Self {
            radius: 50.0,
            theta: 45f32.to_radians(),
            phi: 60f32.to_radians(),
            target: Vec3::ZERO,
            follow_drone: true,
            mode: CameraMode::Overhead,
            eye_height: 2.0,
        }
    }
}

fn setup_scene(mut commands: Commands) {
    use bevy::pbr::FogFalloff;

    commands.spawn((
        Camera3dBundle {
            transform: Transform::from_xyz(20.0, 30.0, 20.0).looking_at(Vec3::ZERO, Vec3::Y),
            ..default()
        },
        OrbitCamera::default(),
        bevy::pbr::FogSettings {
            color: Color::srgb(0.35, 0.55, 0.8),
            falloff: FogFalloff::Linear {
                start: 300.0,
                end: 2000.0,
            },
            ..default()
        },
    ));
}

/// Orbit camera controller for the main viewport.
/// Middle-click or Ctrl+Left-drag to orbit (swivel), right-click to pan.
/// Left-click is free for EGUI interactions.
fn camera_controller(
    time: Res<Time>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    mut mouse_motion: EventReader<bevy::input::mouse::MouseMotion>,
    mut mouse_wheel: EventReader<bevy::input::mouse::MouseWheel>,
    mut query: Query<(&mut Transform, &mut OrbitCamera), With<Camera3d>>,
    drone_query: Query<&drone::Kinematics>,
    mut camera_mode: ResMut<CameraMode>,
) {
    let orbit_speed = 0.005;
    let pan_speed = 0.5;
    let zoom_speed = 2.0;
    let key_speed = 20.0 * time.delta_seconds();

    let motions: Vec<_> = mouse_motion.read().collect();
    let wheels: Vec<_> = mouse_wheel.read().collect();

    for (mut transform, mut orbit) in query.iter_mut() {
        if keyboard.just_pressed(KeyCode::KeyF) {
            orbit.follow_drone = !orbit.follow_drone;
        }
        if keyboard.just_pressed(KeyCode::KeyV) {
            orbit.mode = match orbit.mode {
                CameraMode::Overhead => CameraMode::StreetView,
                CameraMode::StreetView => CameraMode::Overhead,
            };
            *camera_mode = orbit.mode;
            println!("Camera mode switched to: {:?}", orbit.mode);
        }

        let drone_pos = drone_query.iter().next().map(|k| k.position);

        static mut FRAME_COUNT: u32 = 0;
        unsafe {
            FRAME_COUNT += 1;
            if FRAME_COUNT % 300 == 0 {
                println!("Camera: mode={:?}, pos={:.1},{:.1},{:.1}, target={:.1},{:.1},{:.1}",
                    orbit.mode,
                    transform.translation.x, transform.translation.y, transform.translation.z,
                    orbit.target.x, orbit.target.y, orbit.target.z);
            }
        }

        match orbit.mode {
            CameraMode::Overhead => {
                if orbit.follow_drone {
                    if let Some(target_pos) = drone_pos {
                        if target_pos.is_finite() {
                            let lerp_factor = 5.0 * time.delta_seconds();
                            orbit.target = orbit.target.lerp(target_pos, lerp_factor.min(1.0));
                        }
                    }
                }

                let is_orbiting = mouse_button.pressed(MouseButton::Middle)
                    || (keyboard.pressed(KeyCode::ControlLeft) && mouse_button.pressed(MouseButton::Left));
                if is_orbiting {
                    for motion in &motions {
                        orbit.theta -= motion.delta.x * orbit_speed;
                        orbit.phi = (orbit.phi + motion.delta.y * orbit_speed)
                            .clamp(5f32.to_radians(), 175f32.to_radians());
                    }
                }

                let is_panning = mouse_button.pressed(MouseButton::Right);
                if is_panning {
                    orbit.follow_drone = false;
                    for motion in &motions {
                        let right = transform.right();
                        let up = transform.up();
                        orbit.target -= right * motion.delta.x * pan_speed;
                        orbit.target += up * motion.delta.y * pan_speed;
                    }
                }

                let zoom_input: f32 = wheels.iter().map(|w| w.y).sum();
                if zoom_input != 0.0 {
                    orbit.radius -= zoom_input * zoom_speed;
                    orbit.radius = orbit.radius.clamp(5.0, 500.0);
                }

                if keyboard.pressed(KeyCode::ArrowUp) {
                    orbit.follow_drone = false;
                    orbit.target += Vec3::Y * key_speed;
                }
                if keyboard.pressed(KeyCode::ArrowDown) {
                    orbit.follow_drone = false;
                    orbit.target -= Vec3::Y * key_speed;
                    if orbit.target.y < 0.5 {
                        orbit.target.y = 0.5;
                    }
                }
                if keyboard.pressed(KeyCode::ArrowLeft) {
                    orbit.follow_drone = false;
                    orbit.target -= Vec3::X * key_speed;
                }
                if keyboard.pressed(KeyCode::ArrowRight) {
                    orbit.follow_drone = false;
                    orbit.target += Vec3::X * key_speed;
                }
                if keyboard.pressed(KeyCode::PageUp) || keyboard.pressed(KeyCode::Equal) {
                    orbit.radius -= key_speed;
                    orbit.radius = orbit.radius.max(5.0);
                }
                if keyboard.pressed(KeyCode::PageDown) || keyboard.pressed(KeyCode::Minus) {
                    orbit.radius += key_speed;
                    orbit.radius = orbit.radius.min(500.0);
                }

                let x = orbit.radius * orbit.phi.sin() * orbit.theta.cos();
                let y = orbit.radius * orbit.phi.cos();
                let z = orbit.radius * orbit.phi.sin() * orbit.theta.sin();
                transform.translation = orbit.target + Vec3::new(x, y, z);
                transform.look_at(orbit.target, Vec3::Y);
            }
            CameraMode::StreetView => {
                if orbit.follow_drone {
                    if let Some(target_pos) = drone_pos {
                        if target_pos.is_finite() {
                            let lerp_factor = 5.0 * time.delta_seconds();
                            orbit.target.x = orbit.target.x.lerp(target_pos.x, lerp_factor.min(1.0));
                            orbit.target.z = orbit.target.z.lerp(target_pos.z, lerp_factor.min(1.0));
                        }
                    }
                }

                if mouse_button.pressed(MouseButton::Middle) || mouse_button.pressed(MouseButton::Right) {
                    for motion in &motions {
                        orbit.theta -= motion.delta.x * orbit_speed;
                        orbit.phi = (orbit.phi + motion.delta.y * orbit_speed)
                            .clamp(10f32.to_radians(), 170f32.to_radians());
                    }
                }

                let zoom_input: f32 = wheels.iter().map(|w| w.y).sum();
                if zoom_input != 0.0 {
                    orbit.eye_height += zoom_input * 0.5;
                    orbit.eye_height = orbit.eye_height.clamp(1.5, 50.0);
                }

                if keyboard.pressed(KeyCode::ArrowUp) || keyboard.pressed(KeyCode::KeyW) {
                    orbit.follow_drone = false;
                    let forward = Vec3::new(orbit.theta.sin(), 0.0, orbit.theta.cos());
                    orbit.target += forward * key_speed;
                }
                if keyboard.pressed(KeyCode::ArrowDown) || keyboard.pressed(KeyCode::KeyS) {
                    orbit.follow_drone = false;
                    let forward = Vec3::new(orbit.theta.sin(), 0.0, orbit.theta.cos());
                    orbit.target -= forward * key_speed;
                }
                if keyboard.pressed(KeyCode::ArrowLeft) || keyboard.pressed(KeyCode::KeyA) {
                    orbit.follow_drone = false;
                    let right = Vec3::new(orbit.theta.cos(), 0.0, -orbit.theta.sin());
                    orbit.target -= right * key_speed;
                }
                if keyboard.pressed(KeyCode::ArrowRight) || keyboard.pressed(KeyCode::KeyD) {
                    orbit.follow_drone = false;
                    let right = Vec3::new(orbit.theta.cos(), 0.0, -orbit.theta.sin());
                    orbit.target += right * key_speed;
                }
                if keyboard.pressed(KeyCode::PageUp) || keyboard.pressed(KeyCode::Equal) {
                    orbit.eye_height += key_speed * 0.5;
                    orbit.eye_height = orbit.eye_height.min(50.0);
                }
                if keyboard.pressed(KeyCode::PageDown) || keyboard.pressed(KeyCode::Minus) {
                    orbit.eye_height -= key_speed * 0.5;
                    orbit.eye_height = orbit.eye_height.max(1.5);
                }

                let cam_pos = Vec3::new(orbit.target.x, orbit.eye_height.max(1.5), orbit.target.z);
                let look_dir = Vec3::new(
                    orbit.phi.sin() * orbit.theta.sin(),
                    orbit.phi.cos(),
                    orbit.phi.sin() * orbit.theta.cos(),
                );
                transform.translation = cam_pos;
                transform.look_at(cam_pos + look_dir, Vec3::Y);
            }
        }
    }
}

pub fn handle_reload_world(
    mut reload_events: EventReader<ReloadWorldEvent>,
    mut despawn_events: EventWriter<DespawnWorldEvent>,
    mut reload_osm_events: EventWriter<ReloadOsmEvent>,
    mut clear_drone_events: EventWriter<ClearDroneDataEvent>,
    mut respawn_events: EventWriter<RespawnDronesEvent>,
) {
    for event in reload_events.read() {
        despawn_events.send(DespawnWorldEvent);
        clear_drone_events.send(ClearDroneDataEvent);
        reload_osm_events.send(ReloadOsmEvent {
            lat: event.lat,
            lon: event.lon,
        });
        respawn_events.send(RespawnDronesEvent);
    }
}

fn sync_minimap_center(
    geo: Res<crate::core::gps::GeoReference>,
    mut tile_state: ResMut<ui::map_tiles::MapTileState>,
) {
    let (px, py) = crate::core::mercator::lat_lon_to_pixel(geo.origin_lat, geo.origin_lon, tile_state.zoom);
    tile_state.center_pixel_x = px;
    tile_state.center_pixel_y = py;
}

fn debug_navigate_hotkey(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut events: EventWriter<ReloadWorldEvent>,
) {
    if keyboard.just_pressed(KeyCode::KeyN) {
        events.send(ReloadWorldEvent {
            lat: 37.7749,
            lon: -122.4194,
        });
    }
}
