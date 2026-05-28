use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPlugin, EguiSet};
use crate::core::types::{DroneId, DroneType, FlightMode};
use crate::camera::CameraMode;
use crate::drone::{DroneIdentity, FleetId, FleetRegistry, Kinematics, GpsPosition, FlightControl, Battery, Health, DamageLevel, MissionState, EnvironmentSettings, SpawnDroneEvent, controller::{DroneInputState, process_input, update_flight_mode}, camera::DroneCameraMap};
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

#[derive(Resource)]
pub struct UiPreferences {
    pub llm_enabled: bool,
    pub selected_spawn_type: DroneType,
    pub spawn_lat: String,
    pub spawn_lon: String,
    pub spawn_alt: String,
    pub formation_idx: usize,
    pub formation_spread: f32,
    pub formation_active: bool,
}

impl Default for UiPreferences {
    fn default() -> Self {
        Self {
            llm_enabled: false,
            selected_spawn_type: DroneType::MavicStyle,
            spawn_lat: "37.7749".to_string(),
            spawn_lon: "-122.4194".to_string(),
            spawn_alt: "50".to_string(),
            formation_idx: 0,
            formation_spread: 10.0,
            formation_active: false,
        }
    }
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
    pub environment: bool,
    pub camera_feed_show_all: bool,
    pub camera_feed_fullscreen: bool,
    pub map: bool,
    pub show_airspace: bool,
    pub show_mission: bool,
    pub show_config: bool,
    pub help: bool,
}

impl Default for PanelVisibility {
    fn default() -> Self {
        Self {
            drone_control: true,
            telemetry: false,
            llm_reasoning: false,
            metrics: false,
            navigate: false,
            camera_feeds: true,
            mission_planner: false,
            environment: false,
            camera_feed_show_all: false,
            camera_feed_fullscreen: false,
            map: true,
            show_airspace: false,
            show_mission: false,
            show_config: false,
            help: false,
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
            .add_systems(Update, environment_panel.run_if(resource_exists::<crate::splash::AppReady>).after(EguiSet::InitContexts))
            .add_systems(Update, config_panel.run_if(resource_exists::<crate::splash::AppReady>).after(EguiSet::InitContexts))
            .add_systems(Update, map_tiles::map_panel.run_if(resource_exists::<crate::splash::AppReady>).after(EguiSet::InitContexts))
            .add_systems(Update, map_tiles::apply_map_drag.run_if(resource_exists::<crate::splash::AppReady>))
            .add_systems(Update, help_panel.run_if(resource_exists::<crate::splash::AppReady>).after(EguiSet::InitContexts))
            .add_systems(Update, clear_minimap_on_reload.after(crate::handle_reload_world))
            .add_systems(Update, cleanup_tile_download_tasks.after(crate::handle_reload_world));
    }
}

fn top_menu_bar(
    mut contexts: EguiContexts,
    mut panel_vis: ResMut<PanelVisibility>,
    camera_mode: Res<CameraMode>,
) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let hours = (secs / 3600) % 24;
    let minutes = (secs / 60) % 60;
    let seconds = secs % 60;
    let time_str = format!("UTC {:02}:{:02}:{:02}", hours, minutes, seconds);

    egui::TopBottomPanel::top("top_menu").show(contexts.ctx_mut(), |ui| {
        egui::menu::bar(ui, |ui| {
            let toggle = |ui: &mut egui::Ui, label: &str, active: &mut bool| {
                if ui.selectable_label(*active, label).clicked() {
                    *active = !*active;
                }
            };

            toggle(ui, "Drones", &mut panel_vis.drone_control);
            toggle(ui, "Telemetry", &mut panel_vis.telemetry);
            toggle(ui, "Camera", &mut panel_vis.camera_feeds);
            toggle(ui, "Mission", &mut panel_vis.mission_planner);
            toggle(ui, "Mission IO", &mut panel_vis.show_mission);
            toggle(ui, "Map", &mut panel_vis.map);
            toggle(ui, "Nav", &mut panel_vis.navigate);
            toggle(ui, "Env", &mut panel_vis.environment);
            toggle(ui, "LLM", &mut panel_vis.llm_reasoning);
            toggle(ui, "Metrics", &mut panel_vis.metrics);
            toggle(ui, "Airspace", &mut panel_vis.show_airspace);
            toggle(ui, "Config", &mut panel_vis.show_config);
            toggle(ui, "Help", &mut panel_vis.help);

            ui.separator();

            let mode_label = match *camera_mode {
                CameraMode::Overhead => "Overhead",
                CameraMode::StreetView => "Street View",
            };
            ui.label(format!("Cam: {}", mode_label));

            ui.separator();

            ui.label(egui::RichText::new(&time_str).strong().color(egui::Color32::YELLOW));
        });
    });
}

pub fn clear_minimap_on_reload(
    mut events: EventReader<crate::events::ReloadOsmEvent>,
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
    mut events: EventReader<crate::events::DespawnWorldEvent>,
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
    mut fleet_registry: ResMut<FleetRegistry>,
    mut rename_events: EventWriter<crate::drone::RenameDroneEvent>,
    mut serial_events: EventWriter<crate::drone::ChangeSerialEvent>,
    mut spawn_events: EventWriter<SpawnDroneEvent>,
    geo: Res<crate::core::gps::GeoReference>,
) {
    if !panel_vis.drone_control {
        return;
    }
    let mut drone_control_open = true;
    egui::Window::new("Drone Control")
        .default_pos([10.0, 40.0])
        .default_size([280.0, 200.0])
        .open(&mut drone_control_open)
        .collapsible(true)
        .default_open(false)
        .show(contexts.ctx_mut(), |ui| {
        ui.set_width(276.0);

        if input_state.input_captured {
            ui.colored_label(egui::Color32::from_rgb(255, 180, 60),
                "⚠ Keyboard captured (Esc to release)");
        } else {
            ui.colored_label(egui::Color32::from_rgb(100, 200, 100), "✓ Keyboard active");
        }

        // ── Spawn (collapsible) ──
        ui.collapsing("Spawn Drone", |ui| {
            ui.horizontal(|ui| {
                ui.label("Type:");
                egui::ComboBox::from_id_source("spawn_type")
                    .selected_text(format!("{:?}", ui_state.selected_spawn_type))
                    .show_ui(ui, |ui| {
                        for dt in [DroneType::TinyWhoop, DroneType::Racing5Inch, DroneType::Racing7Inch, DroneType::CineWhoop, DroneType::MavicStyle, DroneType::InspireStyle, DroneType::Hexacopter, DroneType::Octocopter, DroneType::VtolFixedWing, DroneType::ToyDrone] {
                            ui.selectable_value(&mut ui_state.selected_spawn_type, dt, format!("{:?}", dt));
                        }
                    });
            });
            if ui_state.spawn_lat.is_empty() {
                ui_state.spawn_lat = format!("{:.6}", geo.origin_lat);
                ui_state.spawn_lon = format!("{:.6}", geo.origin_lon);
                ui_state.spawn_alt = "20.0".to_string();
            }
            ui.horizontal(|ui| {
                ui.add_sized([40.0, 20.0], egui::Label::new("Lat:"));
                ui.add_sized([80.0, 20.0], egui::TextEdit::singleline(&mut ui_state.spawn_lat));
                ui.add_sized([40.0, 20.0], egui::Label::new("Lon:"));
                ui.add_sized([80.0, 20.0], egui::TextEdit::singleline(&mut ui_state.spawn_lon));
            });
            ui.horizontal(|ui| {
                ui.add_sized([40.0, 20.0], egui::Label::new("Alt:"));
                ui.add_sized([60.0, 20.0], egui::TextEdit::singleline(&mut ui_state.spawn_alt));
            });
            let spawn_btn = egui::Button::new("SPAWN").min_size(egui::vec2(260.0, 28.0));
            if ui.add(spawn_btn).clicked() {
                if let (Ok(lat), Ok(lon), Ok(alt)) = (
                    ui_state.spawn_lat.parse(),
                    ui_state.spawn_lon.parse(),
                    ui_state.spawn_alt.parse(),
                ) {
                    spawn_events.send(SpawnDroneEvent { lat, lon, alt, drone_type: ui_state.selected_spawn_type });
                }
            }
        });

        ui.checkbox(&mut ui_state.llm_enabled, "LLM Autopilot");

        // ── Flight Modes (compact grid) ──
        ui.horizontal(|ui| {
            ui.label("Mode:");
            for (key, mode, label) in [
                ("1", FlightMode::Manual, "Manual"),
                ("2", FlightMode::Stabilize, "Stab"),
                ("3", FlightMode::AltHold, "AltH"),
                ("4", FlightMode::Guided, "Guide"),
                ("5", FlightMode::Auto, "Auto"),
                ("6", FlightMode::Rtl, "RTL"),
                ("7", FlightMode::Land, "Land"),
            ] {
                if ui.add_sized([32.0, 20.0], egui::Button::new(format!("{}:{}", key, label))).clicked() {
                    input_state.mode_request = Some(mode);
                }
            }
        });

        // ── Fleet (collapsible) ──
        ui.collapsing("Fleet", |ui| {
            let fleet_ids: Vec<FleetId> = fleet_registry.fleets.keys().copied().collect();
            let active = fleet_registry.active_fleet;
            let fleet_names: Vec<String> = fleet_ids.iter().map(|fid| {
                fleet_registry.fleets.get(fid).map(|f| f.name.clone()).unwrap_or_default()
            }).collect();

            ui.horizontal(|ui| {
                egui::ComboBox::from_id_source("active_fleet")
                    .selected_text(fleet_registry.fleets.get(&active).map(|f| f.name.clone()).unwrap_or_default())
                    .show_ui(ui, |ui| {
                        for (i, fid) in fleet_ids.iter().enumerate() {
                            if ui.selectable_label(*fid == active, &fleet_names[i]).clicked() {
                                fleet_registry.set_active(*fid);
                            }
                        }
                    });
                if ui.button("+").clicked() {
                    fleet_registry.create_fleet(format!("Fleet {}", fleet_ids.len() + 1));
                }
                if fleet_ids.len() > 1 && ui.button("-").clicked() {
                    fleet_registry.remove_fleet(active);
                }
            });

            if let Some(active_fleet) = fleet_registry.fleets.get(&active) {
                let mut fleet_name = active_fleet.name.clone();
                ui.add_sized([200.0, 18.0], egui::TextEdit::singleline(&mut fleet_name));
            }

            ui.horizontal(|ui| {
                let drones: Vec<_> = drone_query.iter().collect();
                let drone_labels: Vec<String> = drones.iter().map(|(id, fc, ..)| {
                    format!("{} ({:?})", id.name, fc.mode)
                }).collect();
                if !drone_labels.is_empty() {
                    let selected_idx = drones.iter().position(|(id, ..)| fleet_registry.is_selected(id.id)).unwrap_or(0);
                    egui::ComboBox::from_id_source("select_drone")
                        .selected_text(&drone_labels[selected_idx])
                        .show_ui(ui, |ui| {
                            for (i, label) in drone_labels.iter().enumerate() {
                                if ui.selectable_label(i == selected_idx, label).clicked() {
                                    fleet_registry.select_single(drones[i].0.id);
                                }
                            }
                        });
                }
                if ui.button("All").clicked() {
                    fleet_registry.select_all();
                }
            });

            ui.separator();
            ui.label("Formation");
            let formations = ["None", "Line", "Diamond", "V-Shape", "Circle"];
            egui::ComboBox::from_id_source("formation")
                .selected_text(formations[ui_state.formation_idx])
                .show_ui(ui, |ui| {
                    for (i, name) in formations.iter().enumerate() {
                        if ui.selectable_label(i == ui_state.formation_idx, *name).clicked() {
                            ui_state.formation_idx = i;
                        }
                    }
                });
            ui.add(egui::Slider::new(&mut ui_state.formation_spread, 5.0..=50.0).text("Spread (m)"));
            if ui.button("Apply Formation").clicked() {
                ui_state.formation_active = ui_state.formation_idx > 0;
            }

            for (identity, flight_control, health, battery) in drone_query.iter() {
                let is_selected = fleet_registry.is_selected(identity.id);
                let serial_str = identity.serial.as_deref().unwrap_or("");
                let status_color = if !health.is_operational {
                    egui::Color32::DARK_GRAY
                } else {
                    match health.damage_level {
                        DamageLevel::None => egui::Color32::GREEN,
                        DamageLevel::Minor => egui::Color32::YELLOW,
                        DamageLevel::Major => egui::Color32::from_rgb(255, 140, 0),
                        _ => egui::Color32::RED,
                    }
                };
                ui.horizontal(|ui| {
                    ui.colored_label(
                        if is_selected { egui::Color32::GREEN } else { egui::Color32::LIGHT_GRAY },
                        format!("{}{}", identity.name, if serial_str.is_empty() { String::new() } else { format!(" [{}]", serial_str) }),
                    );
                    ui.colored_label(status_color, format!("{:.0}%hp", health.health_percent));
                    ui.colored_label(egui::Color32::from_rgb(100, 200, 255), format!("{:.0}%bat", battery.percent));
                    ui.label(format!("{:?}", flight_control.mode));
                });
                if is_selected {
                    let rename_id = ui.id().with(("ren", identity.id));
                    let mut buf = ui.memory(|mem| {
                        mem.data.get_temp::<String>(rename_id).unwrap_or_else(|| identity.name.clone())
                    });
                    let resp = ui.add_sized([130.0, 16.0], egui::TextEdit::singleline(&mut buf).hint_text("Rename..."));
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        rename_events.send(crate::drone::RenameDroneEvent { drone_id: identity.id, new_name: buf.clone() });
                    }
                    ui.memory_mut(|mem| { mem.data.insert_temp(rename_id, buf); });

                    let serial_id = ui.id().with(("ser", identity.id));
                    let serial_str = identity.serial.clone().unwrap_or_default();
                    let mut ser_buf = ui.memory(|mem| {
                        mem.data.get_temp::<String>(serial_id).unwrap_or(serial_str)
                    });
                    let ser_resp = ui.add_sized([130.0, 16.0], egui::TextEdit::singleline(&mut ser_buf).hint_text("Serial #..."));
                    if ser_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        let new_serial = if ser_buf.is_empty() { None } else { Some(ser_buf.clone()) };
                        serial_events.send(crate::drone::ChangeSerialEvent { drone_id: identity.id, serial: new_serial });
                    }
                    ui.memory_mut(|mem| { mem.data.insert_temp(serial_id, ser_buf); });
                }
            }
        });

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("W/S=Fwd/Back  A/D=Left/Right  Space/Shift=Up/Down  Q/E=Yaw").text_style(egui::TextStyle::Small).weak());
        });
    });
}

fn telemetry_panel(
    mut contexts: EguiContexts,
    drone_query: Query<(&DroneIdentity, &Kinematics, &GpsPosition, &Health, &Battery, Option<&crate::drone::physics::AirspaceViolation>)>,
    mut kml_exporter: ResMut<crate::export::KmlExporter>,
    panel_vis: Res<PanelVisibility>,
) {
    if !panel_vis.telemetry {
        return;
    }
    let Some(ctx) = contexts.try_ctx_mut() else {
        return;
    };
    egui::Window::new("Telemetry")
        .default_pos([10.0, 240.0])
        .default_size([320.0, 180.0])
        .show(ctx, |ui| {
        for (identity, kinematics, gps, health, battery, violation_opt) in drone_query.iter() {
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

                let battery_color = if battery.percent > 50.0 {
                    egui::Color32::GREEN
                } else if battery.percent > 25.0 {
                    egui::Color32::YELLOW
                } else if battery.percent > 10.0 {
                    egui::Color32::from_rgb(255, 140, 0)
                } else {
                    egui::Color32::RED
                };
                ui.horizontal(|ui| {
                    ui.label("Battery:");
                    ui.colored_label(battery_color, format!("{:.1}% ({:.0}mAh)", battery.percent, battery.current_charge_mah));
                });

                if let Some(violation) = violation_opt {
                    if violation.in_no_fly {
                        ui.colored_label(egui::Color32::RED, format!("⚠ NO-FLY ZONE: {}", violation.zone_name));
                    } else if violation.in_height_restricted {
                        ui.colored_label(egui::Color32::from_rgb(255, 140, 0), format!("⚠ HEIGHT RESTRICTED: {} (max {}ft)", violation.zone_name, violation.max_altitude_ft.map(|a| format!("{:.0}", a)).unwrap_or_else(|| "?".to_string())));
                    }
                }

                if health.last_impact_velocity > 0.1 {
                    ui.label(format!("Last Impact: {:.1} m/s", health.last_impact_velocity));
                }
                if !health.is_operational {
                    ui.colored_label(egui::Color32::RED, "NOT OPERATIONAL");
                }
                ui.label(format!("Type: {:?}", identity.drone_type));
            });
        }

        ui.separator();
        ui.heading("KML Export");
        if kml_exporter.is_exporting {
            ui.horizontal(|ui| {
                ui.colored_label(egui::Color32::GREEN, "● Recording");
                if ui.button("Stop").clicked() {
                    kml_exporter.stop_export();
                }
            });
        } else {
            if ui.button("Start KML Export").clicked() {
                use chrono::Local;
                let ts = Local::now().format("%Y%m%d_%H%M%S");
                let path = format!("data/exports/{}_flight.kml", ts);
                let _ = std::fs::create_dir_all("data/exports");
                kml_exporter.start_export(path);
            }
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
    mut panel_vis: ResMut<PanelVisibility>,
    fleet_registry: Res<FleetRegistry>,
) {
    if !panel_vis.camera_feeds {
        return;
    }
    let mut textures_to_show = Vec::new();
    for identity in drone_query.iter() {
        let is_selected = fleet_registry.is_selected(identity.id);
        if !panel_vis.camera_feed_show_all && !is_selected {
            continue;
        }
        if let Some(handle) = camera_map.images.get(&identity.id) {
            let texture_id = contexts.add_image(handle.clone());
            textures_to_show.push((identity.name.clone(), identity.serial.clone(), texture_id, is_selected));
        }
    }

    let fullscreen = panel_vis.camera_feed_fullscreen;
    let (default_pos, default_size, image_size) = if fullscreen {
        ([20.0, 20.0], [1240.0, 840.0], [1200.0, 750.0])
    } else {
        ([660.0, 40.0], [320.0, 240.0], [280.0, 180.0])
    };

    egui::Window::new("Camera Feed")
        .default_pos(default_pos)
        .default_size(default_size)
        .show(contexts.ctx_mut(), |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(&mut panel_vis.camera_feed_show_all, "Show all drones");
                if ui.button(if fullscreen { "Exit Fullscreen" } else { "Fullscreen" }).clicked() {
                    panel_vis.camera_feed_fullscreen = !fullscreen;
                }
            });
            ui.separator();
            if textures_to_show.is_empty() {
                ui.label("No drone selected. Select a drone to see its camera.");
            }
            for (name, serial, texture_id, is_selected) in &textures_to_show {
                let label = if let Some(sn) = serial {
                    format!("{} (S/N: {})", name, sn)
                } else {
                    name.clone()
                };
                let rich = if *is_selected {
                    egui::RichText::new(label).color(egui::Color32::GREEN).strong()
                } else {
                    egui::RichText::new(label).weak()
                };
                ui.label(rich);
                ui.image(egui::load::SizedTexture::new(*texture_id, image_size));
                ui.separator();
            }
        });
}

fn mission_panel(
    mut contexts: EguiContexts,
    mut state: ResMut<MissionPlannerState>,
    mut drone_query: Query<(&DroneIdentity, &mut MissionState, &mut FlightControl)>,
    fleet_registry: Res<FleetRegistry>,
    panel_vis: Res<PanelVisibility>,
) {
    if !panel_vis.mission_planner {
        return;
    }
    let Some(ctx) = contexts.try_ctx_mut() else {
        return;
    };
    use crate::core::types::{Mission, Waypoint};

    egui::Window::new("Mission Planner")
        .default_pos([660.0, 450.0])
        .default_size([320.0, 300.0])
        .show(ctx, |ui| {
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
                if !state.waypoints.is_empty() {
                    let selected_ids: Vec<DroneId> = fleet_registry.selected().iter().copied().collect();
                    for (identity, mut mission_state, mut flight_control) in drone_query.iter_mut() {
                        if selected_ids.contains(&identity.id) {
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

fn environment_panel(
    mut contexts: EguiContexts,
    mut env: ResMut<EnvironmentSettings>,
    panel_vis: Res<PanelVisibility>,
) {
    if !panel_vis.environment {
        return;
    }
    egui::Window::new("Environment")
        .default_pos([660.0, 20.0])
        .default_size([280.0, 200.0])
        .show(contexts.ctx_mut(), |ui| {
            ui.heading("Wind & Atmosphere");

            ui.add(egui::Slider::new(&mut env.wind_speed_ms, 0.0..=50.0)
                .text("Wind Speed (m/s)"));
            ui.add(egui::Slider::new(&mut env.wind_direction_deg, 0.0..=360.0)
                .text("Wind Dir (°)"));
            ui.add(egui::Slider::new(&mut env.turbulence, 0.0..=1.0)
                .text("Turbulence"));
            ui.add(egui::Slider::new(&mut env.gust_factor, 0.0..=1.0)
                .text("Gust Factor"));
            ui.add(egui::Slider::new(&mut env.rain_intensity, 0.0..=1.0)
                .text("Rain Intensity"));
            ui.add(egui::Slider::new(&mut env.icing_factor, 0.0..=1.0)
                .text("Icing Factor"));

            ui.separator();
            ui.label("Atmosphere");
            ui.add(egui::Slider::new(&mut env.sea_level_density, 0.5..=1.5)
                .text("Sea Level Density"));
            ui.add(egui::Slider::new(&mut env.density_scale_height_m, 1000.0..=20000.0)
                .text("Scale Height (m)"));

            ui.separator();
            if env.wind_speed_ms > 0.0 {
                let dir = env.wind_direction_deg;
                let dir_label = if dir < 22.5 || dir >= 337.5 { "N" }
                    else if dir < 67.5 { "NE" }
                    else if dir < 112.5 { "E" }
                    else if dir < 157.5 { "SE" }
                    else if dir < 202.5 { "S" }
                    else if dir < 247.5 { "SW" }
                    else if dir < 292.5 { "W" }
                    else { "NW" };
                ui.label(format!("Wind: {:.1} m/s from {:.0}° ({})", env.wind_speed_ms, dir, dir_label));
            } else {
                ui.label("Calm");
            }
        });
}

fn config_panel(
    mut contexts: EguiContexts,
    mut physics_config: ResMut<crate::core::config::PhysicsConfig>,
    mut llm_config: ResMut<crate::core::config::LlmConfig>,
    mut env: ResMut<EnvironmentSettings>,
    panel_vis: Res<PanelVisibility>,
) {
    if !panel_vis.show_config {
        return;
    }
    egui::Window::new("Config Editor")
        .default_pos([340.0, 20.0])
        .default_size([300.0, 400.0])
        .show(contexts.ctx_mut(), |ui| {
            ui.heading("Physics");
            ui.add(egui::Slider::new(&mut physics_config.wind_speed_ms, 0.0..=50.0).text("Wind Speed (m/s)"));
            ui.add(egui::Slider::new(&mut physics_config.wind_direction_deg, 0.0..=360.0).text("Wind Direction (°)"));
            ui.add(egui::Slider::new(&mut physics_config.turbulence, 0.0..=1.0).text("Turbulence"));
            ui.add(egui::Slider::new(&mut physics_config.gust_factor, 0.0..=1.0).text("Gust Factor"));
            ui.add(egui::Slider::new(&mut physics_config.rain_intensity, 0.0..=1.0).text("Rain Intensity"));
            ui.add(egui::Slider::new(&mut physics_config.icing_factor, 0.0..=1.0).text("Icing Factor"));
            ui.add(egui::Slider::new(&mut physics_config.sea_level_density, 0.5..=1.5).text("Sea Level Density"));

            ui.separator();
            ui.heading("LLM");
            ui.checkbox(&mut llm_config.enabled, "Enabled");
            ui.add(egui::Slider::new(&mut llm_config.temperature, 0.0..=2.0).text("Temperature"));
            ui.horizontal(|ui| {
                ui.label("Provider:");
                ui.text_edit_singleline(&mut llm_config.provider);
            });
            ui.horizontal(|ui| {
                ui.label("Model:");
                ui.text_edit_singleline(&mut llm_config.model);
            });

            ui.separator();
            if ui.button("Save to sim.toml").clicked() {
                let sim_config = crate::core::config::SimConfig {
                    physics: physics_config.clone(),
                    llm: llm_config.clone(),
                    ..Default::default()
                };
                if let Err(e) = crate::core::config::save_config(std::path::Path::new("config/sim.toml"), &sim_config) {
                    eprintln!("Failed to save config: {}", e);
                }
            }

            env.wind_speed_ms = physics_config.wind_speed_ms;
            env.wind_direction_deg = physics_config.wind_direction_deg;
            env.turbulence = physics_config.turbulence;
            env.gust_factor = physics_config.gust_factor;
            env.rain_intensity = physics_config.rain_intensity;
            env.icing_factor = physics_config.icing_factor;
            env.sea_level_density = physics_config.sea_level_density;
        });
}

fn help_panel(mut contexts: EguiContexts, panel_vis: Res<PanelVisibility>) {
    if !panel_vis.help {
        return;
    }
    egui::Window::new("Keyboard Shortcuts")
        .default_pos([340.0, 40.0])
        .default_size([380.0, 520.0])
        .show(contexts.ctx_mut(), |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("Drone Flight");
                ui.separator();
                shortcut_row(ui, "W / S", "Forward / Backward");
                shortcut_row(ui, "A / D", "Left / Right");
                shortcut_row(ui, "Space / Shift", "Ascend / Descend");
                shortcut_row(ui, "Q / E", "Yaw Left / Right");
                shortcut_row(ui, "1 - 7", "Flight Modes (Manual..Land)");
                shortcut_row(ui, "X", "Emergency Stop (Land)");

                ui.add_space(12.0);
                ui.heading("Camera");
                ui.separator();
                shortcut_row(ui, "F", "Toggle Follow Drone");
                shortcut_row(ui, "V", "Toggle Overhead / Street View");
                shortcut_row(ui, "R", "Reset Camera to Default");
                shortcut_row(ui, "Mouse Middle + Drag", "Orbit Camera");
                shortcut_row(ui, "Mouse Right + Drag", "Pan Camera");
                shortcut_row(ui, "Scroll", "Zoom In / Out");
                shortcut_row(ui, "Arrow Keys", "Keyboard Pan");
                shortcut_row(ui, "PgUp / PgDn", "Zoom In / Out");
                shortcut_row(ui, "+ / -", "Zoom In / Out");

                ui.add_space(12.0);
                ui.heading("UI Panels");
                ui.separator();
                shortcut_row(ui, "F1", "Toggle Help");
                shortcut_row(ui, "F2", "Toggle Drone Control");
                shortcut_row(ui, "F3", "Toggle Telemetry");
                shortcut_row(ui, "F4", "Toggle Camera Feeds");
                shortcut_row(ui, "F5", "Toggle Map");
                shortcut_row(ui, "F6", "Toggle LLM Reasoning");
                shortcut_row(ui, "F7", "Toggle Metrics");
                shortcut_row(ui, "F9", "Toggle Environment");
                shortcut_row(ui, "F11", "Camera Feed Fullscreen");

                ui.add_space(12.0);
                ui.heading("Simulation");
                ui.separator();
                shortcut_row(ui, "P", "Pause / Resume");
                shortcut_row(ui, "[ / ]", "Decrease / Increase Time Scale");
                shortcut_row(ui, "Tab", "Toggle All UI Panels");
                shortcut_row(ui, "N", "Navigate to San Francisco");
                shortcut_row(ui, "Escape", "Unfocus UI Text Fields");

                ui.add_space(12.0);
                ui.heading("Flight Modes");
                ui.separator();
                shortcut_row(ui, "1", "Manual");
                shortcut_row(ui, "2", "Stabilize");
                shortcut_row(ui, "3", "AltHold");
                shortcut_row(ui, "4", "Guided");
                shortcut_row(ui, "5", "Auto");
                shortcut_row(ui, "6", "RTL (Return to Launch)");
                shortcut_row(ui, "7", "Land");
            });
        });
}

fn shortcut_row(ui: &mut egui::Ui, key: &str, action: &str) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(key).monospace().strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(action);
        });
    });
}
