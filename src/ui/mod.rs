use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPlugin, EguiSet};
use crate::core::types::{DroneType, FlightMode, CameraMode};
use crate::drone::{DroneIdentity, Kinematics, GpsPosition, FlightControl, Battery, Health, DamageLevel, MissionState, controller::{DroneInputState, process_input, update_flight_mode}, camera::DroneCameraMap};
use crate::eval::metrics::MetricsCollector;
use crate::eval::trace::TraceCollector;
use crate::llm::reasoning::ReasoningTrace;

pub mod map_tiles;

#[derive(Event)]
pub struct ReloadWorldEvent {
    pub lat: f64,
    pub lon: f64,
}

#[derive(Resource, Default)]
pub struct NavigateState {
    pub lat_input: String,
    pub lon_input: String,
}

#[derive(Resource, Default)]
pub struct MissionPlannerState {
    pub waypoints: Vec<crate::core::types::Waypoint>,
    pub lat_input: String,
    pub lon_input: String,
    pub alt_input: String,
    pub loop_mission: bool,
}

#[derive(Resource, Default)]
pub struct UiPreferences {
    pub llm_enabled: bool,
    pub selected_spawn_type: DroneType,
}

#[derive(Resource)]
pub struct PanelVisibility {
    pub drone_control: bool,
    pub telemetry: bool,
    pub llm_reasoning: bool,
    pub metrics: bool,
    pub navigate: bool,
    pub camera_feeds: bool,
    pub mission_planner: bool,
}

impl Default for PanelVisibility {
    fn default() -> Self {
        Self {
            drone_control: true,
            telemetry: true,
            llm_reasoning: true,
            metrics: true,
            navigate: true,
            camera_feeds: true,
            mission_planner: true,
        }
    }
}

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        use bevy::prelude::resource_exists;
        app.add_plugins(EguiPlugin)
            .add_plugins(map_tiles::MapTilePlugin)
            .init_resource::<NavigateState>()
            .init_resource::<MissionPlannerState>()
            .init_resource::<UiPreferences>()
            .init_resource::<PanelVisibility>()
            .add_event::<ReloadWorldEvent>()
            .add_systems(Update, top_menu_bar.run_if(resource_exists::<crate::splash::AppReady>).after(EguiSet::InitContexts))
            .add_systems(Update, drone_panel.run_if(resource_exists::<crate::splash::AppReady>).after(process_input).before(update_flight_mode).after(EguiSet::InitContexts))
            .add_systems(Update, telemetry_panel.run_if(resource_exists::<crate::splash::AppReady>).after(EguiSet::InitContexts))
            .add_systems(Update, llm_panel.run_if(resource_exists::<crate::splash::AppReady>).after(EguiSet::InitContexts))
            .add_systems(Update, metrics_panel.run_if(resource_exists::<crate::splash::AppReady>).after(EguiSet::InitContexts))
            .add_systems(Update, navigate_panel.run_if(resource_exists::<crate::splash::AppReady>).after(EguiSet::InitContexts))
            .add_systems(Update, camera_feed_panel.run_if(resource_exists::<crate::splash::AppReady>).after(EguiSet::InitContexts))
            .add_systems(Update, mission_panel.run_if(resource_exists::<crate::splash::AppReady>).after(EguiSet::InitContexts))
            .add_systems(Update, map_tiles::map_panel.run_if(resource_exists::<crate::splash::AppReady>).after(EguiSet::InitContexts))
            .add_systems(Update, clear_minimap_on_reload.after(crate::handle_reload_world))
            .add_systems(Update, cleanup_tile_download_tasks.after(crate::handle_reload_world));
    }
}

fn top_menu_bar(
    mut contexts: EguiContexts,
    mut panel_vis: ResMut<PanelVisibility>,
    camera_mode: Res<CameraMode>,
) {
    egui::TopBottomPanel::top("top_menu").show(contexts.ctx_mut(), |ui| {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("View", |ui| {
                ui.checkbox(&mut panel_vis.drone_control, "Drone Control");
                ui.checkbox(&mut panel_vis.telemetry, "Telemetry");
                ui.checkbox(&mut panel_vis.llm_reasoning, "LLM Reasoning");
                ui.checkbox(&mut panel_vis.metrics, "Metrics");
                ui.checkbox(&mut panel_vis.navigate, "Navigate");
                ui.checkbox(&mut panel_vis.camera_feeds, "Camera Feeds");
                ui.checkbox(&mut panel_vis.mission_planner, "Mission Planner");
            });

            ui.separator();

            let mode_label = match *camera_mode {
                CameraMode::Overhead => "Overhead",
                CameraMode::StreetView => "Street View",
            };
            ui.label(format!("Camera: {}", mode_label));
            ui.label("(V to toggle)");
        });
    });
}

pub fn clear_minimap_on_reload(
    mut events: EventReader<crate::ReloadOsmEvent>,
    mut tile_state: ResMut<map_tiles::MapTileState>,
) {
    for event in events.read() {
        let (px, py) = crate::core::mercator::lat_lon_to_pixel(event.lat, event.lon, tile_state.zoom);
        tile_state.center_pixel_x = px;
        tile_state.center_pixel_y = py;
        tile_state.tiles.clear();
    }
}

pub fn cleanup_tile_download_tasks(
    mut events: EventReader<crate::DespawnWorldEvent>,
    mut commands: Commands,
    minimap_tasks: Query<Entity, With<map_tiles::TileDownloadTask>>,
) {
    for _ in events.read() {
        for entity in minimap_tasks.iter() {
            commands.entity(entity).despawn();
        }
    }
}

fn drone_panel(
    mut contexts: EguiContexts,
    mut input_state: ResMut<DroneInputState>,
    mut ui_state: ResMut<UiPreferences>,
    panel_vis: Res<PanelVisibility>,
    drone_query: Query<(&DroneIdentity, &FlightControl, &Health, &Battery)>,
) {
    if !panel_vis.drone_control {
        return;
    }
    egui::Window::new("Drone Control")
        .default_pos([10.0, 40.0])
        .default_size([320.0, 180.0])
        .show(contexts.ctx_mut(), |ui| {
        ui.heading("Flight Modes");
        ui.horizontal(|ui| {
            if ui.button("1: Manual").clicked() { input_state.mode_request = Some(FlightMode::Manual); }
            if ui.button("2: Stabilize").clicked() { input_state.mode_request = Some(FlightMode::Stabilize); }
            if ui.button("3: AltHold").clicked() { input_state.mode_request = Some(FlightMode::AltHold); }
        });
        ui.horizontal(|ui| {
            if ui.button("4: Guided").clicked() { input_state.mode_request = Some(FlightMode::Guided); }
            if ui.button("5: Auto").clicked() { input_state.mode_request = Some(FlightMode::Auto); }
            if ui.button("6: RTL").clicked() { input_state.mode_request = Some(FlightMode::Rtl); }
            if ui.button("7: Land").clicked() { input_state.mode_request = Some(FlightMode::Land); }
        });

        ui.separator();
        ui.checkbox(&mut ui_state.llm_enabled, "LLM Autopilot");

        ui.separator();
        ui.heading("Active Drone");
        let drones: Vec<_> = drone_query.iter().collect();
        let mut selected_idx = drones.iter().position(|(id, ..)| input_state.selected_drone == Some(id.id)).unwrap_or(0);
        let drone_labels: Vec<String> = drones.iter().map(|(id, fc, ..)| {
            let id_str = format!("{:?}", id.id.0);
            format!("Drone {} ({:?})", &id_str[..id_str.len().min(8)], fc.mode)
        }).collect();
        if !drone_labels.is_empty() {
            egui::ComboBox::from_label("Select")
                .selected_text(&drone_labels[selected_idx])
                .show_ui(ui, |ui| {
                    for (i, label) in drone_labels.iter().enumerate() {
                        if ui.selectable_label(i == selected_idx, label).clicked() {
                            selected_idx = i;
                            input_state.selected_drone = Some(drones[i].0.id);
                        }
                    }
                });
        }

        ui.separator();
        egui::ComboBox::from_label("Spawn Type")
            .selected_text(format!("{:?}", ui_state.selected_spawn_type))
            .show_ui(ui, |ui| {
                for dt in [DroneType::TinyWhoop, DroneType::Racing5Inch, DroneType::Racing7Inch, DroneType::CineWhoop, DroneType::MavicStyle, DroneType::InspireStyle, DroneType::Hexacopter, DroneType::Octocopter, DroneType::VtolFixedWing, DroneType::ToyDrone] {
                    ui.selectable_value(&mut ui_state.selected_spawn_type, dt, format!("{:?}", dt));
                }
            });

        ui.separator();
        ui.heading("Fleet Status");
        for (identity, flight_control, health, battery) in drone_query.iter() {
            let id_str = format!("{:?}", identity.id.0);
            let is_selected = input_state.selected_drone.map(|id| id == identity.id).unwrap_or(true);
            let type_str = format!("{:?}", identity.drone_type);
            let label = if is_selected {
                egui::RichText::new(format!("Drone {} ({})", &id_str[..id_str.len().min(8)], type_str))
                    .color(egui::Color32::GREEN)
            } else {
                egui::RichText::new(format!("  Drone {} ({})", &id_str[..id_str.len().min(8)], type_str))
                    .weak()
            };
            ui.horizontal(|ui| {
                ui.label(label);
                let status = if !health.is_operational {
                    egui::RichText::new("DESTROYED").color(egui::Color32::DARK_GRAY)
                } else {
                    match health.damage_level {
                        DamageLevel::None => egui::RichText::new("OK").color(egui::Color32::GREEN),
                        DamageLevel::Minor => egui::RichText::new("MINOR").color(egui::Color32::YELLOW),
                        DamageLevel::Major => egui::RichText::new("MAJOR").color(egui::Color32::from_rgb(255, 140, 0)),
                        DamageLevel::Critical => egui::RichText::new("CRITICAL").color(egui::Color32::RED),
                        DamageLevel::Destroyed => egui::RichText::new("DESTROYED").color(egui::Color32::DARK_GRAY),
                    }
                };
                ui.label(status);
            });
            ui.horizontal(|ui| {
                ui.add(egui::ProgressBar::new(health.health_percent / 100.0)
                    .text(format!("Health: {:.0}%", health.health_percent))
                    .desired_width(120.0));
                ui.label(format!("Bat: {:.0}%", battery.percent));
                ui.label(format!("{:?}", flight_control.mode));
            });
        }

        ui.separator();
        ui.label("Controls: W/S - Forward/Back | A/D - Left/Right");
        ui.label("Space/Shift - Up/Down | Q/E - Yaw");
        ui.label("Camera: Middle-drag = Orbit | Right-drag = Pan | Scroll = Zoom");
    });
}

fn telemetry_panel(
    mut contexts: EguiContexts,
    drone_query: Query<(&DroneIdentity, &Kinematics, &GpsPosition, &Health, &Battery)>,
    panel_vis: Res<PanelVisibility>,
) {
    if !panel_vis.telemetry {
        return;
    }
    egui::Window::new("Telemetry")
        .default_pos([10.0, 240.0])
        .default_size([320.0, 180.0])
        .show(contexts.ctx_mut(), |ui| {
        for (identity, kinematics, gps, health, battery) in drone_query.iter() {
            ui.group(|ui| {
                ui.label(format!("Position: {:.1}, {:.1}, {:.1}",
                    kinematics.position.x, kinematics.position.y, kinematics.position.z));
                ui.label(format!("Velocity: {:.1} m/s", kinematics.velocity.length()));
                ui.label(format!("GPS: {:.6}, {:.6}, {:.1}m",
                    gps.coord.latitude, gps.coord.longitude, gps.coord.altitude_msl));

                let (_, pitch, roll) = kinematics.orientation.to_euler(EulerRot::YXZ);
                ui.label(format!("Attitude: Pitch={:.1}°, Roll={:.1}°",
                    pitch.to_degrees(), roll.to_degrees()));

                ui.separator();
                let health_color = if health.health_percent > 80.0 {
                    egui::Color32::GREEN
                } else if health.health_percent > 50.0 {
                    egui::Color32::YELLOW
                } else if health.health_percent > 20.0 {
                    egui::Color32::from_rgb(255, 140, 0)
                } else {
                    egui::Color32::RED
                };
                ui.horizontal(|ui| {
                    ui.label("Health:");
                    ui.colored_label(health_color, format!("{:.0}%", health.health_percent));
                });
                if health.last_impact_velocity > 0.1 {
                    ui.label(format!("Last Impact: {:.1} m/s", health.last_impact_velocity));
                }
                if !health.is_operational {
                    ui.colored_label(egui::Color32::RED, "NOT OPERATIONAL");
                }
                ui.label(format!("Type: {:?}", identity.drone_type));
                ui.label(format!("Battery: {:.1}% ({:.0}mAh)", battery.percent, battery.current_charge_mah));
            });
        }
    });
}

fn llm_panel(
    mut contexts: EguiContexts,
    reasoning: Res<ReasoningTrace>,
    mut ui_state: ResMut<UiPreferences>,
    panel_vis: Res<PanelVisibility>,
) {
    if !panel_vis.llm_reasoning {
        return;
    }
    egui::Window::new("LLM Reasoning")
        .default_pos([10.0, 430.0])
        .default_size([320.0, 200.0])
        .show(contexts.ctx_mut(), |ui| {
        let llm_status = if ui_state.llm_enabled {
            egui::RichText::new("LLM Autopilot: ON").color(egui::Color32::GREEN)
        } else {
            egui::RichText::new("LLM Autopilot: OFF").color(egui::Color32::RED)
        };
        ui.label(llm_status);
        if ui.button("Toggle LLM").clicked() {
            ui_state.llm_enabled = !ui_state.llm_enabled;
        }
        ui.heading("Active Reasoning Chains");
        if reasoning.active_chains.is_empty() {
            ui.label("No reasoning chains yet. LLM decisions will appear here.");
        }
        for (drone_id, chain) in reasoning.active_chains.iter() {
            let id_str = format!("{:?}", drone_id.0);
            let header_id = ui.id().with(format!("llm_reasoning_{}", id_str));
            egui::CollapsingHeader::new(format!("Drone {}", &id_str[..id_str.len().min(8)]))
                .id_source(header_id)
                .default_open(true)
                .show(ui, |ui| {
                    if chain.steps.is_empty() {
                        ui.label("No reasoning steps yet.");
                    } else {
                        for step in &chain.steps {
                            ui.label(format!("[{}] {}: {} (conf: {:.2})",
                                step.step_type, step.step_id,
                                step.content.chars().take(60).collect::<String>(),
                                step.confidence));
                        }
                    }
                });
        }

        ui.separator();

        let status_id = ui.id().with("reasoning_export_status");
        let mut status: Option<(String, f64)> = ui.memory(|mem| {
            mem.data.get_temp(status_id)
        });

        if ui.button("Export Reasoning").clicked() {
            let json = reasoning.export_to_json();
            let path = std::path::PathBuf::from("data/reasoning_export.json");
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            match std::fs::write(&path, json) {
                Ok(()) => {
                    status = Some((format!("Saved to {}", path.display()), ui.input(|i| i.time)));
                }
                Err(e) => {
                    status = Some((format!("Error: {}", e), ui.input(|i| i.time)));
                }
            }
        }

        if let Some((msg, timestamp)) = &status {
            let elapsed = ui.input(|i| i.time) - *timestamp;
            if elapsed < 3.0 {
                let color = if msg.starts_with("Error") {
                    egui::Color32::RED
                } else {
                    egui::Color32::LIGHT_GREEN
                };
                ui.label(egui::RichText::new(msg).color(color));
            } else {
                status = None;
            }
        }

        ui.memory_mut(|mem| {
            mem.data.insert_temp(status_id, status);
        });
    });
}

fn metrics_panel(
    mut contexts: EguiContexts,
    metrics: Res<MetricsCollector>,
    trace: Res<TraceCollector>,
    panel_vis: Res<PanelVisibility>,
) {
    if !panel_vis.metrics {
        return;
    }
    egui::Window::new("Metrics")
        .default_pos([340.0, 40.0])
        .default_size([300.0, 200.0])
        .show(contexts.ctx_mut(), |ui| {
        if metrics.drone_metrics.is_empty() {
            ui.label("No drone metrics available yet.");
        }
        for (drone_id, m) in metrics.drone_metrics.iter() {
            let id_str = format!("{:?}", drone_id.0);
            let header_id = ui.id().with(format!("metrics_{}", id_str));
            egui::CollapsingHeader::new(format!("Drone {}", &id_str[..id_str.len().min(8)]))
                .id_source(header_id)
                .default_open(true)
                .show(ui, |ui| {
                    ui.label(format!("Distance: {:.1} m", m.total_distance_m));
                    ui.label(format!("Flight Time: {:.1} s", m.flight_time_secs));
                    ui.label(format!("Avg Speed: {:.1} m/s", m.average_speed_ms));
                    ui.label(format!("Max Speed: {:.1} m/s", m.max_speed_ms));
                    ui.label(format!("Altitude Range: {:.1} - {:.1} m", m.min_altitude_m, m.max_altitude_m));
                });
        }

        ui.separator();
        if ui.button("Save Trace").clicked() {
            if let Err(e) = trace.flush() {
                ui.label(format!("Error: {}", e));
            } else {
                ui.label("Trace saved!");
            }
        }
        ui.label(format!("Buffered events: {}",
            trace.event_buffer.lock().map(|b| b.len()).unwrap_or(0)));
    });
}

fn navigate_panel(
    mut contexts: EguiContexts,
    mut state: ResMut<NavigateState>,
    mut events: EventWriter<ReloadWorldEvent>,
    panel_vis: Res<PanelVisibility>,
) {
    if !panel_vis.navigate {
        return;
    }
    egui::Window::new("Navigate")
        .default_pos([340.0, 250.0])
        .default_size([280.0, 120.0])
        .show(contexts.ctx_mut(), |ui| {
            ui.horizontal(|ui| {
                ui.label("Lat:");
                ui.text_edit_singleline(&mut state.lat_input);
            });
            ui.horizontal(|ui| {
                ui.label("Lon:");
                ui.text_edit_singleline(&mut state.lon_input);
            });
            if ui.button("Navigate").clicked() {
                if let (Ok(lat), Ok(lon)) = (state.lat_input.parse(), state.lon_input.parse()) {
                    events.send(ReloadWorldEvent { lat, lon });
                }
            }
        });
}

fn camera_feed_panel(
    mut contexts: EguiContexts,
    drone_query: Query<&DroneIdentity>,
    camera_map: Res<DroneCameraMap>,
    panel_vis: Res<PanelVisibility>,
) {
    if !panel_vis.camera_feeds {
        return;
    }
    let mut textures_to_show = Vec::new();
    for identity in drone_query.iter() {
        if let Some(handle) = camera_map.images.get(&identity.id) {
            let id_str = format!("{:?}", identity.id.0);
            let texture_id = contexts.add_image(handle.clone());
            textures_to_show.push((id_str, texture_id));
        }
    }

    egui::Window::new("Camera Feeds")
        .default_pos([660.0, 40.0])
        .default_size([320.0, 400.0])
        .show(contexts.ctx_mut(), |ui| {
            for (id_str, texture_id) in textures_to_show {
                ui.label(format!("Drone {}", &id_str[..id_str.len().min(8)]));
                let size = [280.0, 180.0];
                ui.image(egui::load::SizedTexture::new(texture_id, size));
                ui.separator();
            }
        });
}

fn mission_panel(
    mut contexts: EguiContexts,
    mut state: ResMut<MissionPlannerState>,
    mut drone_query: Query<(&DroneIdentity, &mut MissionState, &mut FlightControl)>,
    input_state: Res<DroneInputState>,
    panel_vis: Res<PanelVisibility>,
) {
    if !panel_vis.mission_planner {
        return;
    }
    use crate::core::types::{Mission, Waypoint};

    egui::Window::new("Mission Planner")
        .default_pos([660.0, 450.0])
        .default_size([320.0, 300.0])
        .show(contexts.ctx_mut(), |ui| {
            ui.heading("Add Waypoint");
            ui.horizontal(|ui| {
                ui.label("Lat:");
                ui.text_edit_singleline(&mut state.lat_input);
            });
            ui.horizontal(|ui| {
                ui.label("Lon:");
                ui.text_edit_singleline(&mut state.lon_input);
            });
            ui.horizontal(|ui| {
                ui.label("Alt AGL:");
                ui.text_edit_singleline(&mut state.alt_input);
            });
            ui.checkbox(&mut state.loop_mission, "Loop mission");

            if ui.button("Add Waypoint").clicked() {
                if let (Ok(lat), Ok(lon), Ok(alt)) = (
                    state.lat_input.parse(),
                    state.lon_input.parse(),
                    state.alt_input.parse(),
                ) {
                    state.waypoints.push(Waypoint {
                        latitude: lat,
                        longitude: lon,
                        altitude_agl: alt,
                        hold_time_secs: 0.0,
                    });
                    state.lat_input.clear();
                    state.lon_input.clear();
                    state.alt_input.clear();
                }
            }

            ui.separator();
            ui.heading(format!("Waypoints ({})", state.waypoints.len()));
            let mut to_remove = None;
            for (i, wp) in state.waypoints.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("{}: {:.5}, {:.5}, {:.0}m", i + 1, wp.latitude, wp.longitude, wp.altitude_agl));
                    if ui.small_button("x").clicked() {
                        to_remove = Some(i);
                    }
                });
            }
            if let Some(idx) = to_remove {
                state.waypoints.remove(idx);
            }

            ui.separator();
            if ui.button("Assign to Selected Drone").clicked() {
                if let Some(selected_id) = input_state.selected_drone {
                    for (identity, mut mission_state, mut flight_control) in drone_query.iter_mut() {
                        if identity.id == selected_id && !state.waypoints.is_empty() {
                            mission_state.mission = Some(Mission {
                                name: "Planned Mission".to_string(),
                                waypoints: state.waypoints.clone(),
                                loop_mission: state.loop_mission,
                            });
                            mission_state.current_waypoint = 0;
                            flight_control.mode = FlightMode::Auto;
                        }
                    }
                }
            }
            if ui.button("Clear All").clicked() {
                state.waypoints.clear();
            }
        });
}
