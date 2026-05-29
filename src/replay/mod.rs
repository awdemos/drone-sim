use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use crate::drone::{DroneIdentity, Kinematics, GpsPosition, FleetRegistry, DroneDestroyedEvent};
use crate::core::gps::GeoReference;
use crate::core::types::DroneId;

#[derive(Resource, Default)]
pub struct FlightReplay {
    pub loaded_frames: Vec<ReplayFrame>,
    pub playing: bool,
    pub current_frame: usize,
    pub playback_speed: f32,
    pub frame_accumulator: f32,
    pub ghost_entities: HashMap<DroneId, Entity>,
}

#[derive(Debug, Clone)]
pub struct ReplayFrame {
    pub timestamp: f64,
    pub drone_name: String,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub vx: f32,
    pub vy: f32,
    pub vz: f32,
}

#[derive(Component)]
pub struct ReplayGhost {
    pub drone_name: String,
    pub frame_offset: usize,
}

pub fn load_flight_csv(path: &std::path::Path) -> Result<Vec<ReplayFrame>, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let reader = std::io::BufReader::new(file);
    let mut frames = Vec::new();
    let mut headers_seen = false;

    for line in reader.lines() {
        let line = line.map_err(|e| e.to_string())?;
        let line = line.trim();
        if line.is_empty() { continue; }

        if !headers_seen {
            headers_seen = true;
            continue;
        }

        let cols: Vec<&str> = line.split(',').collect();
        if cols.len() < 8 { continue; }

        let timestamp = cols[0].parse::<f64>().unwrap_or(0.0);
        let drone_name = cols[1].to_string();
        let x = cols.get(2).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
        let y = cols.get(3).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
        let z = cols.get(4).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
        let vx = cols.get(5).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
        let vy = cols.get(6).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
        let vz = cols.get(7).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);

        frames.push(ReplayFrame { timestamp, drone_name, x, y, z, vx, vy, vz });
    }

    frames.sort_by(|a, b| a.timestamp.partial_cmp(&b.timestamp).unwrap_or(std::cmp::Ordering::Equal));
    Ok(frames)
}

pub fn replay_system(
    time: Res<Time>,
    mut replay: ResMut<FlightReplay>,
    mut ghost_query: Query<(Entity, &mut ReplayGhost, &mut Kinematics), Without<DroneIdentity>>,
    fleet: Res<FleetRegistry>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !replay.playing || replay.loaded_frames.is_empty() {
        return;
    }

    let dt = time.delta_seconds() * replay.playback_speed;
    replay.frame_accumulator += dt;

    let seconds_per_frame = if replay.loaded_frames.len() > 1 {
        let first = replay.loaded_frames.first().unwrap().timestamp;
        let last = replay.loaded_frames.last().unwrap().timestamp;
        (last - first) / replay.loaded_frames.len() as f64
    } else {
        0.1
    };

    while replay.frame_accumulator >= seconds_per_frame as f32 {
        replay.frame_accumulator -= seconds_per_frame as f32;
        replay.current_frame = (replay.current_frame + 1).min(replay.loaded_frames.len() - 1);
    }

    let frame_idx = replay.current_frame;
    let mut names_at_frame: HashMap<String, (f32, f32, f32)> = HashMap::new();

    let mut i = frame_idx;
    while i > 0 && names_at_frame.len() < 20 {
        if let Some(f) = replay.loaded_frames.get(i) {
            names_at_frame.entry(f.drone_name.clone()).or_insert((f.x, f.y, f.z));
        }
        if i == 0 { break; }
        i -= 1;
    }
    for i in frame_idx..replay.loaded_frames.len().min(frame_idx + 20) {
        if let Some(f) = replay.loaded_frames.get(i) {
            names_at_frame.entry(f.drone_name.clone()).or_insert((f.x, f.y, f.z));
        }
    }

    let active_names: std::collections::HashSet<String> = names_at_frame.keys().cloned().collect();

    for (name, (x, y, z)) in &names_at_frame {
        let mut found = false;
        for (_, mut ghost, mut kin) in ghost_query.iter_mut() {
            if ghost.drone_name == *name {
                kin.position = Vec3::new(*x, *y, *z);
                found = true;
                break;
            }
        }
        if !found {
            let entity = commands.spawn((
                ReplayGhost { drone_name: name.clone(), frame_offset: 0 },
                Kinematics {
                    position: Vec3::new(*x, *y, *z),
                    orientation: Quat::IDENTITY,
                    velocity: Vec3::ZERO,
                    angular_velocity: Vec3::ZERO,
                },
                PbrBundle {
                    mesh: meshes.add(Cuboid::new(0.3, 0.1, 0.3)),
                    material: materials.add(StandardMaterial {
                        base_color: Color::srgba(0.0, 1.0, 0.0, 0.4),
                        alpha_mode: AlphaMode::Blend,
                        ..default()
                    }),
                    transform: Transform::from_xyz(*x, *y, *z),
                    visibility: Visibility::Visible,
                    ..default()
                },
            )).id();
            replay.ghost_entities.insert(DroneId::new(), entity);
        }
    }

    if replay.current_frame >= replay.loaded_frames.len() - 1 {
        replay.playing = false;
    }
}

pub fn replay_panel(
    mut ctx: EguiContexts,
    mut replay: ResMut<FlightReplay>,
) {
    egui::Window::new("Flight Replay").show(ctx.ctx_mut(), |ui| {
        ui.horizontal(|ui| {
            ui.label("CSV Path:");
        });
        ui.horizontal(|ui| {
            if ui.button("Load CSV").clicked() {
                if let Ok(frames) = load_flight_csv(std::path::Path::new("data/flight_logs/latest_flight_log.csv")) {
                    replay.loaded_frames = frames;
                    replay.current_frame = 0;
                    replay.playing = false;
                }
            }
        });

        ui.separator();

        let total = replay.loaded_frames.len();
        let frame_label = if total > 0 {
            format!("Frame: {} / {}", replay.current_frame, total - 1)
        } else {
            "No data loaded".to_string()
        };
        ui.label(&frame_label);

        if total > 0 {
            let mut progress = if total > 1 {
                replay.current_frame as f32 / (total - 1) as f32
            } else {
                0.0
            };
            if ui.add(egui::Slider::new(&mut progress, 0.0..=1.0).show_value(false)).changed() {
                replay.current_frame = (progress * (total - 1) as f32) as usize;
            }
        }

        ui.horizontal(|ui| {
            if ui.button("⏮").clicked() {
                replay.current_frame = 0;
                replay.playing = false;
            }
            if replay.playing {
                if ui.button("⏸").clicked() {
                    replay.playing = false;
                }
            } else {
                if ui.button("▶").clicked() && total > 0 {
                    replay.playing = true;
                    replay.frame_accumulator = 0.0;
                }
            }
            if ui.button("⏭").clicked() {
                replay.current_frame = total.saturating_sub(1);
                replay.playing = false;
            }
        });

        ui.horizontal(|ui| {
            ui.label("Speed:");
            ui.add(egui::Slider::new(&mut replay.playback_speed, 0.1..=5.0).text("x"));
        });

        if total > 0 {
            if let Some(f) = replay.loaded_frames.get(replay.current_frame) {
                ui.separator();
                ui.label(format!("Time: {:.2}s", f.timestamp));
                ui.label(format!("Drone: {}", f.drone_name));
                ui.label(format!("Pos: ({:.1}, {:.1}, {:.1})", f.x, f.y, f.z));
                ui.label(format!("Vel: ({:.1}, {:.1}, {:.1})", f.vx, f.vy, f.vz));
            }
        }
    });
}

pub struct ReplayPlugin;

impl Plugin for ReplayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FlightReplay>()
            .add_systems(Update, replay_system)
            .add_systems(Update, replay_panel.after(bevy_egui::EguiSet::InitContexts));
    }
}
