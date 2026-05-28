use bevy::prelude::*;
use std::path::Path;
use crate::core::types::Mission;

pub struct MissionPlugin;

impl Plugin for MissionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MissionIOState>()
            .add_systems(Update, mission_ui);
    }
}

#[derive(Resource, Default)]
pub struct MissionIOState {
    pub status_message: Option<String>,
}

pub fn save_mission_to_file(mission: &Mission, path: &Path) -> Result<(), String> {
    let json = serde_json::to_string_pretty(mission).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn load_mission_from_file(path: &Path) -> Result<Mission, String> {
    let data = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mission: Mission = serde_json::from_str(&data).map_err(|e| e.to_string())?;
    Ok(mission)
}

pub fn list_mission_files() -> Vec<String> {
    let dir = Path::new("data/missions");
    if !dir.exists() {
        return Vec::new();
    }
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name.ends_with(".json") {
                    files.push(name.to_string());
                }
            }
        }
    }
    files.sort();
    files
}

pub fn ensure_missions_dir() {
    let _ = std::fs::create_dir_all("data/missions");
}

use bevy_egui::egui;
use crate::ui::{PanelVisibility, MissionPlannerState};

pub fn mission_ui(
    mut contexts: bevy_egui::EguiContexts,
    mut panel_vis: ResMut<PanelVisibility>,
    mut planner: ResMut<MissionPlannerState>,
    mut io_state: ResMut<MissionIOState>,
) {
    if !panel_vis.show_mission {
        return;
    }

    egui::Window::new("Mission Save/Load")
        .open(&mut panel_vis.show_mission)
        .show(contexts.ctx_mut(), |ui| {
            ui.label(format!("Waypoints: {}", planner.waypoints.len()));

            if ui.button("Save Mission").clicked() {
                ensure_missions_dir();
                let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
                let path = Path::new("data/missions").join(format!("{}.json", timestamp));
                let mission = Mission {
                    name: format!("Mission {}", timestamp),
                    waypoints: planner.waypoints.clone(),
                    loop_mission: planner.loop_mission,
                };
                match save_mission_to_file(&mission, &path) {
                    Ok(()) => io_state.status_message = Some(format!("Saved to {}", path.display())),
                    Err(e) => io_state.status_message = Some(format!("Save failed: {}", e)),
                }
            }

            ui.separator();

            let files = list_mission_files();
            if files.is_empty() {
                ui.label("No saved missions.");
            } else {
                ui.label("Load Mission:");
                for file in &files {
                    if ui.button(file).clicked() {
                        let path = Path::new("data/missions").join(file);
                        match load_mission_from_file(&path) {
                            Ok(m) => {
                                planner.waypoints = m.waypoints;
                                planner.loop_mission = m.loop_mission;
                                io_state.status_message = Some(format!("Loaded {}", file));
                            }
                            Err(e) => io_state.status_message = Some(format!("Load failed: {}", e)),
                        }
                    }
                }
            }

            if let Some(msg) = &io_state.status_message {
                ui.separator();
                ui.colored_label(egui::Color32::GREEN, msg);
            }
        });
}
