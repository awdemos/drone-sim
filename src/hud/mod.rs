use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use crate::drone::{DroneIdentity, Kinematics, Battery, FlightControl, FleetRegistry};
use crate::drone::sensors::{GpsNoise, SensorNoise};
use crate::core::types::FlightMode;

#[derive(Resource, Default)]
pub struct HudState {
    pub show_hud: bool,
    pub show_sensor_panel: bool,
    pub altitude_warning_threshold: f32,
    pub battery_warning_threshold: f32,
    pub last_beep_time: f64,
    pub beep_interval: f64,
}

pub fn hud_overlay(
    mut ctx: EguiContexts,
    time: Res<Time>,
    hud_state: Res<HudState>,
    query: Query<(
        &DroneIdentity,
        &Kinematics,
        &Battery,
        &FlightControl,
        Option<&GpsNoise>,
        Option<&SensorNoise>,
    )>,
    fleet: Res<FleetRegistry>,
    geo: Res<crate::core::gps::GeoReference>,
) {
    if !hud_state.show_hud { return; }

    let selected_id = fleet.active().selected.iter().next().copied();
    let selected_entity = selected_id.and_then(|id| fleet.active().drones.get(&id).copied());

    let Some(entity) = selected_entity else { return };
    let Ok((identity, kinematics, battery, fc, gps_noise, sensor_noise)) = query.get(entity) else { return };

    let screen = ctx.ctx_mut().available_rect();
    let hud_width = 260.0;
    let hud_height = 320.0;
    let margin = 10.0;

    egui::Area::new(egui::Id::new("hud_overlay"))
        .fixed_pos(egui::Pos2::new(screen.right() - hud_width - margin, margin))
        .order(egui::Order::Foreground)
        .show(ctx.ctx_mut(), |ui| {
            egui::Frame::none()
                .fill(egui::Color32::from_black_alpha(180))
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(0, 200, 0)))
                .rounding(4.0)
                .inner_margin(8.0)
                .show(ui, |ui| {
                    ui.set_min_size(egui::Vec2::new(hud_width, hud_height));

                    let color_green = egui::Color32::from_rgb(0, 255, 0);
                    let color_yellow = egui::Color32::from_rgb(255, 255, 0);
                    let color_red = egui::Color32::from_rgb(255, 50, 50);

                    let alt = kinematics.position.y;
                    let speed = kinematics.velocity.length();
                    let v_speed = kinematics.velocity.y;
                    let (yaw, pitch, roll) = kinematics.orientation.to_euler(EulerRot::YXZ);

                    ui.colored_label(color_green, format!("═══ {} ═══", identity.name));
                    ui.separator();

                    let alt_color = if alt < hud_state.altitude_warning_threshold { color_red }
                                    else if alt < hud_state.altitude_warning_threshold * 2.0 { color_yellow }
                                    else { color_green };
                    ui.colored_label(alt_color, format!("ALT  {:>8.1} m", alt));
                    ui.colored_label(color_green, format!("SPD  {:>8.1} m/s", speed));
                    ui.colored_label(color_green, format!("V/S  {:>8.1} m/s", v_speed));

                    let heading = yaw.to_degrees() % 360.0;
                    ui.colored_label(color_green, format!("HDG  {:>8.1}°", if heading < 0.0 { heading + 360.0 } else { heading }));
                    ui.colored_label(color_green, format!("PIT  {:>8.1}°", pitch.to_degrees()));
                    ui.colored_label(color_green, format!("ROL  {:>8.1}°", roll.to_degrees()));

                    ui.separator();

                    let batt_color = if battery.percent < hud_state.battery_warning_threshold { color_red }
                                     else if battery.percent < hud_state.battery_warning_threshold * 2.0 { color_yellow }
                                     else { color_green };
                    ui.colored_label(batt_color, format!("BAT  {:>8.1}% ({:.1}V)", battery.percent, battery.voltage));
                    ui.colored_label(color_green, format!("MODE {:?}", fc.mode));

                    if let Some(gn) = gps_noise {
                        ui.separator();
                        let fix_color = if gn.fix_lost { color_red } else { color_green };
                        ui.colored_label(fix_color, format!("GPS  {} SAT", gn.num_satellites));
                        ui.colored_label(color_green, format!("HDOP {:>8.1}", gn.hdop));
                    }

                    if let Some(sn) = sensor_noise {
                        ui.colored_label(color_green, format!("BARO {:>8.1} m", sn.last_baro_altitude));
                        ui.colored_label(color_green, format!("MAG  {:>8.1}°", sn.last_mag_heading));
                    }

                    if alt < hud_state.altitude_warning_threshold {
                        ui.separator();
                        ui.colored_label(color_red, "⚠ LOW ALTITUDE");
                    }
                    if battery.percent < hud_state.battery_warning_threshold {
                        ui.colored_label(color_red, "⚠ LOW BATTERY");
                    }
                    if gps_noise.map_or(false, |gn| gn.fix_lost) {
                        ui.colored_label(color_red, "⚠ GPS FIX LOST");
                    }
                });
        });
}

pub fn sensor_panel(
    mut ctx: EguiContexts,
    query: Query<(
        &DroneIdentity,
        &Kinematics,
        Option<&GpsNoise>,
        Option<&SensorNoise>,
    )>,
    fleet: Res<FleetRegistry>,
) {
    let selected_id = fleet.active().selected.iter().next().copied();
    let selected_entity = selected_id.and_then(|id| fleet.active().drones.get(&id).copied());
    let Some(entity) = selected_entity else { return };
    let Ok((identity, kinematics, gps_noise, sensor_noise)) = query.get(entity) else { return };

    egui::Window::new("Sensor Data").show(ctx.ctx_mut(), |ui| {
        ui.label(format!("Drone: {}", identity.name));
        ui.separator();

        if let Some(gn) = gps_noise {
            ui.heading("GPS");
            ui.label(format!("Noise stddev: {:.2} m", gn.noise_stddev_m));
            ui.label(format!("Drift: ({:.2}, {:.2}, {:.2})", gn.drift_current.x, gn.drift_current.y, gn.drift_current.z));
            ui.label(format!("HDOP: {:.1}  VDOP: {:.1}", gn.hdop, gn.vdop));
            ui.label(format!("Satellites: {}", gn.num_satellites));
            ui.label(format!("Fix: {}", if gn.fix_lost { "LOST" } else { "3D" }));
            ui.separator();
        }

        if let Some(sn) = sensor_noise {
            ui.heading("IMU");
            ui.label(format!("Accel: ({:.3}, {:.3}, {:.3})", sn.last_accel.x, sn.last_accel.y, sn.last_accel.z));
            ui.label(format!("Gyro:  ({:.4}, {:.4}, {:.4})", sn.last_gyro.x, sn.last_gyro.y, sn.last_gyro.z));

            ui.heading("Barometer");
            ui.label(format!("Altitude: {:.2} m (bias: +{:.2})", sn.last_baro_altitude, sn.barometer_alt_bias));

            ui.heading("Magnetometer");
            ui.label(format!("Heading: {:.1}° (bias: {:.3})", sn.last_mag_heading, sn.magnetometer_bias.x));
        }
    });
}

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HudState>()
            .add_systems(Update, hud_overlay.after(bevy_egui::EguiSet::InitContexts))
            .add_systems(Update, sensor_panel.after(bevy_egui::EguiSet::InitContexts));
    }
}
