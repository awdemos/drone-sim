use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use crate::drone::{DroneIdentity, Kinematics, GpsPosition, FlightControl, Battery, Health, PidState};
use crate::drone::sensors::{GpsNoise, SensorNoise};
use crate::core::types::{FlightMode, DroneId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScenarioType {
    #[default]
    None,
    GpsFailure,
    LostLink,
    LowBattery,
    MotorFailure,
    WindGust,
    SensorDegradation,
    AllSystemsFail,
}

impl std::fmt::Display for ScenarioType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScenarioType::None => write!(f, "None"),
            ScenarioType::GpsFailure => write!(f, "GPS Failure"),
            ScenarioType::LostLink => write!(f, "Lost Link"),
            ScenarioType::LowBattery => write!(f, "Low Battery"),
            ScenarioType::MotorFailure => write!(f, "Motor Failure"),
            ScenarioType::WindGust => write!(f, "Wind Gust"),
            ScenarioType::SensorDegradation => write!(f, "Sensor Degradation"),
            ScenarioType::AllSystemsFail => write!(f, "All Systems Fail"),
        }
    }
}

#[derive(Resource, Default)]
pub struct ScenarioState {
    pub active_scenario: ScenarioType,
    pub timer: Timer,
    pub duration_secs: f32,
    pub result: Option<ScenarioResult>,
}

#[derive(Debug, Clone)]
pub struct ScenarioResult {
    pub scenario: ScenarioType,
    pub passed: bool,
    pub duration_secs: f32,
    pub drone_survived: bool,
    pub max_altitude_error: f32,
    pub max_position_error: f32,
    pub notes: String,
}

pub fn run_scenario(
    mut scenario: ResMut<ScenarioState>,
    time: Res<Time>,
    mut drone_query: Query<(
        &DroneIdentity,
        &mut Kinematics,
        &mut FlightControl,
        &mut Battery,
        &mut Health,
        Option<&mut GpsNoise>,
        Option<&mut SensorNoise>,
    ), With<DroneIdentity>>,
    mut env: ResMut<crate::drone::EnvironmentSettings>,
) {
    if scenario.active_scenario == ScenarioType::None {
        return;
    }

    scenario.timer.tick(time.delta());
    scenario.duration_secs += time.delta_seconds();

    match scenario.active_scenario {
        ScenarioType::GpsFailure => {
            for (_, _, _, _, _, mut gps_noise, _) in drone_query.iter_mut() {
                if let Some(ref mut gn) = gps_noise {
                    gn.fix_lost = true;
                    gn.fix_lost_duration = scenario.timer.duration().as_secs_f32() + 10.0;
                }
            }
        }
        ScenarioType::LostLink => {
            for (_, _, mut fc, _, _, _, _) in drone_query.iter_mut() {
                fc.target_velocity = Vec3::ZERO;
                fc.thrust = 0.0;
            }
        }
        ScenarioType::LowBattery => {
            for (_, _, _, mut battery, _, _, _) in drone_query.iter_mut() {
                battery.percent = 8.0;
                battery.voltage = 13.2;
            }
        }
        ScenarioType::MotorFailure => {
            for (_, mut kin, _, _, _, _, _) in drone_query.iter_mut() {
                kin.velocity.y -= 0.5 * time.delta_seconds();
            }
        }
        ScenarioType::WindGust => {
            env.wind_speed_ms = 20.0;
            env.gust_factor = 3.0;
            env.turbulence = 5.0;
        }
        ScenarioType::SensorDegradation => {
            for (_, _, _, _, _, _, mut sensor) in drone_query.iter_mut() {
                if let Some(ref mut s) = sensor {
                    s.imu_accel_noise_stddev = 0.5;
                    s.imu_gyro_noise_stddev = 0.02;
                    s.barometer_noise_stddev = 2.0;
                    s.magnetometer_noise_stddev = 0.05;
                }
            }
        }
        ScenarioType::AllSystemsFail => {
            for (_, _, _, mut battery, mut health, mut gps_noise, mut sensor) in drone_query.iter_mut() {
                battery.percent = 5.0;
                health.health_percent = 30.0;
                health.damage_level = crate::drone::DamageLevel::Minor;
                if let Some(ref mut gn) = gps_noise {
                    gn.fix_lost = true;
                    gn.fix_lost_duration = 60.0;
                }
                if let Some(ref mut s) = sensor {
                    s.imu_accel_noise_stddev = 1.0;
                    s.barometer_noise_stddev = 5.0;
                }
            }
            env.wind_speed_ms = 15.0;
            env.gust_factor = 2.5;
        }
        ScenarioType::None => {}
    }

    if scenario.timer.finished() {
        let mut survived = true;
        for (_, _, _, _, health, _, _) in drone_query.iter() {
            if !health.is_operational {
                survived = false;
                break;
            }
        }

        let passed = survived && drone_query.iter().count() > 0;
        scenario.result = Some(ScenarioResult {
            scenario: scenario.active_scenario,
            passed,
            duration_secs: scenario.duration_secs,
            drone_survived: survived,
            max_altitude_error: 0.0,
            max_position_error: 0.0,
            notes: if passed { "Drone recovered".to_string() } else { "Drone did not survive".to_string() },
        });

        reset_scenario(&mut scenario, &mut env);
    }
}

fn reset_scenario(scenario: &mut ResMut<ScenarioState>, env: &mut ResMut<crate::drone::EnvironmentSettings>) {
    scenario.active_scenario = ScenarioType::None;
    scenario.duration_secs = 0.0;
    env.wind_speed_ms = 5.0;
    env.gust_factor = 1.0;
    env.turbulence = 0.3;
}

pub fn scenario_panel(
    mut ctx: EguiContexts,
    mut scenario: ResMut<ScenarioState>,
    time: Res<Time>,
) {
    egui::Window::new("Scenario Testing").show(ctx.ctx_mut(), |ui| {
        ui.label("Fault Injection Scenarios:");
        ui.separator();

        let scenarios = [
            ScenarioType::GpsFailure,
            ScenarioType::LostLink,
            ScenarioType::LowBattery,
            ScenarioType::MotorFailure,
            ScenarioType::WindGust,
            ScenarioType::SensorDegradation,
            ScenarioType::AllSystemsFail,
        ];

        for s in scenarios {
            ui.horizontal(|ui| {
                let is_active = scenario.active_scenario == s;
                if is_active {
                    ui.colored_label(egui::Color32::YELLOW, "▶");
                }
                if ui.button(s.to_string()).clicked() && scenario.active_scenario == ScenarioType::None {
                    scenario.active_scenario = s;
                    scenario.timer = Timer::from_seconds(15.0, TimerMode::Once);
                    scenario.duration_secs = 0.0;
                    scenario.result = None;
                }
            });
        }

        ui.separator();
        if scenario.active_scenario != ScenarioType::None {
            ui.colored_label(egui::Color32::YELLOW, format!("Running: {} ({:.1}s)", scenario.active_scenario, scenario.duration_secs));
            if scenario.timer.duration().as_secs_f32() > 0.0 {
                let progress = scenario.timer.fraction();
                ui.add(egui::ProgressBar::new(progress).show_percentage());
            }
            if ui.button("Stop").clicked() {
                scenario.active_scenario = ScenarioType::None;
                scenario.duration_secs = 0.0;
            }
        }

        if let Some(ref result) = scenario.result {
            ui.separator();
            let color = if result.passed { egui::Color32::GREEN } else { egui::Color32::RED };
            ui.colored_label(color, format!("Result: {}", if result.passed { "PASS" } else { "FAIL" }));
            ui.label(format!("Scenario: {}", result.scenario));
            ui.label(format!("Duration: {:.1}s", result.duration_secs));
            ui.label(format!("Survived: {}", result.drone_survived));
            ui.label(&result.notes);
        }
    });
}

pub struct ScenarioPlugin;

impl Plugin for ScenarioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ScenarioState>()
            .add_systems(Update, run_scenario.after(crate::drone::DroneSystemSet::Physics))
            .add_systems(Update, scenario_panel.after(bevy_egui::EguiSet::InitContexts));
    }
}
