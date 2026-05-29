mod core;
mod world;
mod drone;
mod llm;
mod eval;
mod ui;
mod splash;
mod camera;
mod events;
mod faa;
mod mission;
mod telemetry;
mod flight;
mod export;
mod replay;
mod scenario;
mod hud;

use bevy::prelude::*;
use bevy::window::{WindowMode, WindowResolution};
use clap::Parser;
use core::config::{load_config, save_default_config};
use core::gps::GeoReference;
use camera::CameraMode;
use world::osm_loader::load_osm_with_fallback;
use world::terrain::insert_terrain_data;
use world::buildings::spawn_buildings_and_roads;
use drone::DronePlugin;
use drone::camera::DroneCameraMap;
use drone::controller::DroneInputState;
use drone::visual::VisualFrameBuffer;
use ui::UiPlugin;
use llm::LlmPlugin;
use llm::reasoning::ReasoningTrace;
use eval::EvalPlugin;
use faa::FaaPlugin;
use mission::MissionPlugin;
use telemetry::TelemetryPlugin;
use flight::FlightPlugin;
use export::ExportPlugin;
use events::*;

#[derive(Parser, Debug)]
#[command(name = "drone-sim")]
#[command(about = "3D Drone Flight Simulator with LLM Integration")]
struct Args {
    #[arg(short, long)]
    config: Option<std::path::PathBuf>,

    #[arg(long)]
    gen_config: bool,

    #[arg(long)]
    lat: Option<f64>,

    #[arg(long)]
    lon: Option<f64>,

    #[arg(short, long, default_value = "1")]
    drones: usize,

    #[arg(long)]
    llm: bool,

    #[arg(long)]
    provider: Option<String>,
}

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

    let geo = GeoReference::new(
        config.world.origin_lat,
        config.world.origin_lon,
        0.0,
    );

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
        .add_plugins(FaaPlugin)
        .add_plugins(MissionPlugin)
        .add_plugins(TelemetryPlugin)
            .add_plugins(FlightPlugin)
.add_plugins(ExportPlugin)
.add_plugins(replay::ReplayPlugin)
.add_plugins(scenario::ScenarioPlugin)
.add_plugins(hud::HudPlugin)
.add_plugins(drone::sensors::SensorPlugin)
.add_plugins(UiPlugin)
        .add_systems(Startup, camera::spawn_main_camera)
        .add_systems(Startup, insert_terrain_data.after(camera::spawn_main_camera))
        .add_systems(Startup, spawn_buildings_and_roads.after(insert_terrain_data))
        .add_systems(Startup, sync_minimap_center.after(camera::spawn_main_camera))
        .add_systems(Update, camera::camera_controller.run_if(resource_exists::<splash::AppReady>))
        .add_systems(Update, handle_reload_world)
        .add_systems(Update, default_location_hotkey)
        .add_systems(Update, simulation_speed_controls.run_if(resource_exists::<splash::AppReady>))
        .add_systems(Update, toggle_ui_panels.run_if(resource_exists::<splash::AppReady>))
        .run();
}

pub fn handle_reload_world(
    mut reload_events: EventReader<ui::ReloadWorldEvent>,
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

fn default_location_hotkey(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut events: EventWriter<ui::ReloadWorldEvent>,
) {
    if keyboard.just_pressed(KeyCode::KeyN) {
        events.send(ui::ReloadWorldEvent {
            lat: 37.7749,
            lon: -122.4194,
        });
    }
}

fn simulation_speed_controls(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut time: ResMut<Time<Virtual>>,
) {
    if keyboard.just_pressed(KeyCode::KeyP) {
        if time.is_paused() {
            time.unpause();
        } else {
            time.pause();
        }
    }
    if keyboard.just_pressed(KeyCode::BracketRight) {
        let new_scale = (time.relative_speed() * 2.0).min(8.0);
        time.set_relative_speed(new_scale);
    }
    if keyboard.just_pressed(KeyCode::BracketLeft) {
        let new_scale = (time.relative_speed() / 2.0).max(0.125);
        time.set_relative_speed(new_scale);
    }
}

fn toggle_ui_panels(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut panel_vis: ResMut<ui::PanelVisibility>,
) {
    if keyboard.just_pressed(KeyCode::Tab) {
        let any_visible = panel_vis.drone_control
            || panel_vis.telemetry
            || panel_vis.camera_feeds
            || panel_vis.map
            || panel_vis.llm_reasoning
            || panel_vis.metrics
            || panel_vis.navigate
            || panel_vis.mission_planner
            || panel_vis.environment
            || panel_vis.show_airspace;
        if any_visible {
            panel_vis.drone_control = false;
            panel_vis.telemetry = false;
            panel_vis.camera_feeds = false;
            panel_vis.map = false;
            panel_vis.llm_reasoning = false;
            panel_vis.metrics = false;
            panel_vis.navigate = false;
            panel_vis.mission_planner = false;
            panel_vis.environment = false;
            panel_vis.show_airspace = false;
        } else {
            panel_vis.drone_control = true;
            panel_vis.telemetry = true;
            panel_vis.camera_feeds = true;
            panel_vis.map = true;
            panel_vis.show_airspace = true;
        }
    }
    if keyboard.just_pressed(KeyCode::F1) {
        panel_vis.help = !panel_vis.help;
    }
    if keyboard.just_pressed(KeyCode::F2) {
        panel_vis.drone_control = !panel_vis.drone_control;
    }
    if keyboard.just_pressed(KeyCode::F3) {
        panel_vis.telemetry = !panel_vis.telemetry;
    }
    if keyboard.just_pressed(KeyCode::F4) {
        panel_vis.camera_feeds = !panel_vis.camera_feeds;
    }
    if keyboard.just_pressed(KeyCode::F5) {
        panel_vis.map = !panel_vis.map;
    }
    if keyboard.just_pressed(KeyCode::F6) {
        panel_vis.llm_reasoning = !panel_vis.llm_reasoning;
    }
    if keyboard.just_pressed(KeyCode::F7) {
        panel_vis.metrics = !panel_vis.metrics;
    }
    if keyboard.just_pressed(KeyCode::F8) {
        panel_vis.show_airspace = !panel_vis.show_airspace;
    }
    if keyboard.just_pressed(KeyCode::F9) {
        panel_vis.environment = !panel_vis.environment;
    }
}
