use bevy::prelude::*;
use std::path::Path;
use crate::core::types::{FlightMode, SimTimestamp};
use crate::drone::{DroneIdentity, Kinematics, GpsPosition, FlightControl, Battery};
use crate::drone::physics::AirspaceViolation;

pub struct TelemetryPlugin;

impl Plugin for TelemetryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FlightRecorder>()
            .add_systems(Update, record_telemetry)
            .add_systems(Update, telemetry_ui);
    }
}

#[derive(Debug, Clone)]
pub struct TelemetryRecord {
    pub timestamp_secs: f64,
    pub drone_id: String,
    pub lat: f64,
    pub lon: f64,
    pub alt_msl: f64,
    pub vx: f32,
    pub vy: f32,
    pub vz: f32,
    pub battery_pct: f32,
    pub flight_mode: FlightMode,
    pub violation: bool,
}

#[derive(Resource, Default)]
pub struct FlightRecorder {
    pub records: Vec<TelemetryRecord>,
    pub is_recording: bool,
    pub frame_counter: u32,
}

impl FlightRecorder {
    pub fn start(&mut self) {
        self.is_recording = true;
        self.records.clear();
        self.frame_counter = 0;
    }

    pub fn stop(&mut self) {
        self.is_recording = false;
    }

    pub fn export_csv(&self) -> Result<String, String> {
        let mut csv = String::from("timestamp,drone_id,lat,lon,alt_msl,vx,vy,vz,battery,mode,violation\n");
        for r in &self.records {
            csv.push_str(&format!(
                "{:.3},{},{:.7},{:.7},{:.2},{:.3},{:.3},{:.3},{:.1},{:?},{}\n",
                r.timestamp_secs, r.drone_id, r.lat, r.lon, r.alt_msl,
                r.vx, r.vy, r.vz, r.battery_pct, r.flight_mode, r.violation
            ));
        }
        Ok(csv)
    }
}

pub fn record_telemetry(
    time: Res<Time>,
    mut recorder: ResMut<FlightRecorder>,
    query: Query<(&DroneIdentity, &Kinematics, &GpsPosition, &FlightControl, &Battery, Option<&AirspaceViolation>)>,
) {
    if !recorder.is_recording {
        return;
    }

    recorder.frame_counter += 1;
    if recorder.frame_counter % 6 != 0 {
        return;
    }

    let timestamp = time.elapsed_seconds_f64();

    for (identity, kinematics, gps, control, battery, violation) in query.iter() {
        let record = TelemetryRecord {
            timestamp_secs: timestamp,
            drone_id: identity.id.0.to_string(),
            lat: gps.coord.latitude,
            lon: gps.coord.longitude,
            alt_msl: gps.coord.altitude_msl,
            vx: kinematics.velocity.x,
            vy: kinematics.velocity.y,
            vz: kinematics.velocity.z,
            battery_pct: battery.percent,
            flight_mode: control.mode,
            violation: violation.map(|v| v.in_no_fly || v.in_height_restricted).unwrap_or(false),
        };
        recorder.records.push(record);
    }
}

use bevy_egui::egui;
use crate::ui::PanelVisibility;

pub fn telemetry_ui(
    mut contexts: bevy_egui::EguiContexts,
    panel_vis: Res<PanelVisibility>,
    mut recorder: ResMut<FlightRecorder>,
) {
    if !panel_vis.telemetry {
        return;
    }

    egui::Window::new("Flight Recorder")
        .show(contexts.ctx_mut(), |ui| {
            if recorder.is_recording {
                ui.horizontal(|ui| {
                    ui.colored_label(egui::Color32::RED, "● REC");
                    ui.label(format!("Records: {}", recorder.records.len()));
                });
                if ui.button("Stop Recording").clicked() {
                    recorder.stop();
                }
            } else {
                if ui.button("Start Recording").clicked() {
                    recorder.start();
                }
            }

            ui.separator();

            if ui.button("Export CSV").clicked() {
                match recorder.export_csv() {
                    Ok(csv) => {
                        let _ = std::fs::create_dir_all("data/flight_logs");
                        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
                        let path = Path::new("data/flight_logs").join(format!("{}_flight_log.csv", timestamp));
                        match std::fs::write(&path, csv) {
                            Ok(()) => {
                                ui.colored_label(egui::Color32::GREEN, format!("Exported to {}", path.display()));
                            }
                            Err(e) => {
                                ui.colored_label(egui::Color32::RED, format!("Export failed: {}", e));
                            }
                        }
                    }
                    Err(e) => {
                        ui.colored_label(egui::Color32::RED, format!("CSV error: {}", e));
                    }
                }
            }
        });
}
