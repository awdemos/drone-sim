use bevy::prelude::*;
use std::collections::HashMap;
use crate::core::types::DroneId;
use crate::drone::{DroneIdentity, Kinematics, GpsPosition};

pub struct FlightPlugin;

impl Plugin for FlightPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PathRecorder>()
            .add_systems(Update, record_paths)
            .add_systems(Update, update_replay_ghosts)
            .add_systems(Update, path_ui);
    }
}

#[derive(Debug, Clone)]
pub struct PathPoint {
    pub position: Vec3,
    pub timestamp: f32,
}

#[derive(Resource, Default)]
pub struct PathRecorder {
    pub paths: HashMap<DroneId, Vec<PathPoint>>,
    pub is_recording: bool,
    pub frame_counter: u32,
}

impl PathRecorder {
    pub fn start_recording(&mut self) {
        self.is_recording = true;
        self.paths.clear();
        self.frame_counter = 0;
    }

    pub fn stop_recording(&mut self) {
        self.is_recording = false;
    }

    pub fn clear_path(&mut self, drone_id: DroneId) {
        self.paths.remove(&drone_id);
    }
}

#[derive(Component)]
pub struct ReplayGhost {
    pub path: Vec<PathPoint>,
    pub current_index: usize,
    pub start_time: f32,
}

pub fn record_paths(
    time: Res<Time>,
    mut recorder: ResMut<PathRecorder>,
    query: Query<(&DroneIdentity, &Kinematics)>,
) {
    if !recorder.is_recording {
        return;
    }

    recorder.frame_counter += 1;
    if recorder.frame_counter % 6 != 0 {
        return;
    }

    let timestamp = time.elapsed_seconds();

    for (identity, kinematics) in query.iter() {
        let point = PathPoint {
            position: kinematics.position,
            timestamp,
        };
        recorder.paths.entry(identity.id).or_default().push(point);
    }
}

pub fn spawn_replay_ghost(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    path: Vec<PathPoint>,
) {
    let mesh = meshes.add(Cuboid::new(0.3, 0.1, 0.3));
    let material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.5, 0.8, 1.0, 0.4),
        alpha_mode: AlphaMode::Blend,
        ..default()
    });

    commands.spawn((
        PbrBundle {
            mesh,
            material,
            transform: Transform::from_translation(path.first().map(|p| p.position).unwrap_or(Vec3::ZERO)),
            ..default()
        },
        ReplayGhost {
            path,
            current_index: 0,
            start_time: 0.0,
        },
    ));
}

pub fn update_replay_ghosts(
    time: Res<Time>,
    mut query: Query<(&mut Transform, &mut ReplayGhost)>,
) {
    let now = time.elapsed_seconds();

    for (mut transform, mut ghost) in query.iter_mut() {
        if ghost.start_time == 0.0 {
            ghost.start_time = now;
        }

        let elapsed = now - ghost.start_time;

        while ghost.current_index + 1 < ghost.path.len() {
            let next = &ghost.path[ghost.current_index + 1];
            if next.timestamp >= elapsed {
                break;
            }
            ghost.current_index += 1;
        }

        if ghost.current_index < ghost.path.len() {
            transform.translation = ghost.path[ghost.current_index].position;
        }
    }
}

use bevy_egui::egui;
use crate::ui::PanelVisibility;

pub fn path_ui(
    mut contexts: bevy_egui::EguiContexts,
    panel_vis: Res<PanelVisibility>,
    mut recorder: ResMut<PathRecorder>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !panel_vis.telemetry {
        return;
    }

    egui::Window::new("Path Replay")
        .show(contexts.ctx_mut(), |ui| {
            if recorder.is_recording {
                ui.horizontal(|ui| {
                    ui.colored_label(egui::Color32::RED, "● REC");
                    ui.label(format!("Paths: {}", recorder.paths.len()));
                });
                if ui.button("Stop").clicked() {
                    recorder.stop_recording();
                }
            } else {
                if ui.button("Record Paths").clicked() {
                    recorder.start_recording();
                }
            }

            ui.separator();

            if ui.button("Replay Ghost").clicked() {
                for path in recorder.paths.values() {
                    if path.len() >= 2 {
                        spawn_replay_ghost(&mut commands, &mut meshes, &mut materials, path.clone());
                    }
                }
            }

            if ui.button("Clear Paths").clicked() {
                recorder.paths.clear();
            }
        });
}
