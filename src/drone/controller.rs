use bevy::prelude::*;
use bevy_egui::EguiContexts;
use crate::core::types::{FlightMode, SimEvent, SimTimestamp};
use crate::core::gps::GeoReference;
use crate::drone::{DroneIdentity, Kinematics, FlightControl, MissionState};
use crate::drone::types::DroneTypeRegistry;
use crate::eval::trace::TraceCollector;
use crate::world::terrain::TerrainData;

/// Keyboard/mouse input state for drone control
#[derive(Resource, Default)]
pub struct DroneInputState {
    pub selected_drone: Option<crate::core::types::DroneId>,
    pub forward: f32,
    pub right: f32,
    pub up: f32,
    pub yaw: f32,
    pub mode_request: Option<FlightMode>,
}

/// Process keyboard input
pub fn process_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut input_state: ResMut<DroneInputState>,
    mut egui_ctx: EguiContexts,
) {
    if egui_ctx.ctx_mut().wants_keyboard_input() {
        return;
    }
    input_state.forward = 0.0;
    input_state.right = 0.0;
    input_state.up = 0.0;
    input_state.yaw = 0.0;

    if keyboard.pressed(KeyCode::KeyW) {
        input_state.forward += 1.0;
    }
    if keyboard.pressed(KeyCode::KeyS) {
        input_state.forward -= 1.0;
    }
    if keyboard.pressed(KeyCode::KeyD) {
        input_state.right += 1.0;
    }
    if keyboard.pressed(KeyCode::KeyA) {
        input_state.right -= 1.0;
    }
    if keyboard.pressed(KeyCode::Space) {
        input_state.up += 1.0;
    }
    if keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight) {
        input_state.up -= 1.0;
    }
    if keyboard.pressed(KeyCode::KeyE) {
        input_state.yaw += 1.0;
    }
    if keyboard.pressed(KeyCode::KeyQ) {
        input_state.yaw -= 1.0;
    }

    // Mode selection
    if keyboard.just_pressed(KeyCode::Digit1) {
        input_state.mode_request = Some(FlightMode::Manual);
    }
    if keyboard.just_pressed(KeyCode::Digit2) {
        input_state.mode_request = Some(FlightMode::Stabilize);
    }
    if keyboard.just_pressed(KeyCode::Digit3) {
        input_state.mode_request = Some(FlightMode::AltHold);
    }
    if keyboard.just_pressed(KeyCode::Digit4) {
        input_state.mode_request = Some(FlightMode::Guided);
    }
    if keyboard.just_pressed(KeyCode::Digit5) {
        input_state.mode_request = Some(FlightMode::Auto);
    }
    if keyboard.just_pressed(KeyCode::Digit6) {
        input_state.mode_request = Some(FlightMode::Rtl);
    }
    if keyboard.just_pressed(KeyCode::Digit7) {
        input_state.mode_request = Some(FlightMode::Land);
    }
}

/// Update drone control based on mode and input
pub fn update_flight_mode(
    time: Res<Time>,
    mut input_state: ResMut<DroneInputState>,
    mut query: Query<(&DroneIdentity, &mut FlightControl, &mut Kinematics, &mut MissionState)>,
    mut trace: ResMut<TraceCollector>,
    geo: Res<GeoReference>,
    terrain: Res<TerrainData>,
    registry: Res<DroneTypeRegistry>,
) {
    let _dt = time.delta_seconds();

    // Process and clear mode change request
    let mode_request = input_state.mode_request.take();
    let selected_id = input_state.selected_drone;

    for (identity, mut flight_control, mut kinematics, mut mission_state) in query.iter_mut() {
        let is_selected = selected_id.map(|id| id == identity.id).unwrap_or(true);

        // Check for mode change request (only for selected drone)
        if is_selected {
            if let Some(requested_mode) = mode_request {
                if requested_mode != flight_control.mode {
                    let old_mode = flight_control.mode;
                    flight_control.mode = requested_mode;
                    trace.record_event(SimEvent::ModeChanged {
                        drone_id: identity.id,
                        timestamp: SimTimestamp::now(),
                        old_mode,
                        new_mode: requested_mode,
                    });
                }
            }
        }

        let spec = registry.specs.get(&identity.drone_type).unwrap();
        let fc = &spec.flight_controller;

        match flight_control.mode {
            FlightMode::Manual => {
                if is_selected {
                    flight_control.thrust = input_state.up.max(0.0) * fc.manual_thrust_scale + fc.manual_thrust_base;
                    flight_control.angular_thrust = Vec3::new(
                        input_state.forward * 0.5,
                        input_state.yaw * 0.5,
                        -input_state.right * 0.5,
                    );
                }
            }
            FlightMode::Stabilize => {
                if is_selected {
                    flight_control.thrust = input_state.up.max(0.0) * fc.manual_thrust_scale + fc.stabilize_thrust_base;
                    let max_tilt = fc.max_tilt_angle_deg.to_radians();
                    let target_pitch = input_state.forward * max_tilt;
                    let target_roll = -input_state.right * max_tilt;
                    let target_yaw_rate = input_state.yaw * 1.0;

                    let (_current_yaw, current_pitch, current_roll) =
                        kinematics.orientation.to_euler(EulerRot::YXZ);

                    flight_control.angular_thrust = Vec3::new(
                        (target_pitch - current_pitch) * fc.pid_gains.attitude_p,
                        target_yaw_rate,
                        (target_roll - current_roll) * fc.pid_gains.attitude_p,
                    );
                }
            }
            FlightMode::AltHold => {
                let target_alt = 10.0f32;
                let alt_error = target_alt - kinematics.position.y;
                flight_control.thrust = fc.stabilize_thrust_base + alt_error * fc.pid_gains.altitude_p;
                flight_control.thrust = flight_control.thrust.clamp(0.3, 0.7);

                if is_selected {
                    let max_tilt = fc.max_tilt_angle_deg.to_radians();
                    let target_pitch = input_state.forward * max_tilt;
                    let target_roll = -input_state.right * max_tilt;
                    let target_yaw_rate = input_state.yaw * 0.5;

                    let (_, current_pitch, current_roll) =
                        kinematics.orientation.to_euler(EulerRot::YXZ);

                    flight_control.angular_thrust = Vec3::new(
                        (target_pitch - current_pitch) * fc.pid_gains.attitude_p,
                        target_yaw_rate,
                        (target_roll - current_roll) * fc.pid_gains.attitude_p,
                    );
                }
            }
            FlightMode::Loiter => {
                let target_vel = Vec3::ZERO;
                let vel_error = target_vel - kinematics.velocity;
                let accel = vel_error * fc.pid_gains.velocity_p;

                flight_control.thrust = fc.stabilize_thrust_base + accel.y * fc.pid_gains.altitude_p;
                flight_control.thrust = flight_control.thrust.clamp(0.3, 0.8);

                let (_, current_pitch, current_roll) =
                    kinematics.orientation.to_euler(EulerRot::YXZ);

                flight_control.angular_thrust = Vec3::new(
                    (accel.z * fc.pid_gains.position_p - current_pitch) * fc.pid_gains.attitude_p,
                    0.0,
                    (-accel.x * fc.pid_gains.position_p - current_roll) * fc.pid_gains.attitude_p,
                );
            }
            FlightMode::Guided => {
                if is_selected {
                    let target_vel = Vec3::new(
                        input_state.right * fc.cruise_speed_ms,
                        input_state.up * fc.max_climb_rate_ms,
                        -input_state.forward * fc.cruise_speed_ms,
                    );
                    let vel_error = target_vel - kinematics.velocity;
                    let accel = vel_error * fc.pid_gains.velocity_p;

                    flight_control.thrust = fc.stabilize_thrust_base + accel.y * fc.pid_gains.altitude_p;
                    flight_control.thrust = flight_control.thrust.clamp(0.3, 0.8);

                    let (_, current_pitch, current_roll) =
                        kinematics.orientation.to_euler(EulerRot::YXZ);

                    flight_control.angular_thrust = Vec3::new(
                        (accel.z * fc.pid_gains.position_p - current_pitch) * fc.pid_gains.attitude_p,
                        input_state.yaw * 0.5,
                        (-accel.x * fc.pid_gains.position_p - current_roll) * fc.pid_gains.attitude_p,
                    );
                }
            }
            FlightMode::Auto => {
                // Follow waypoints
                let mission_len = mission_state.mission.as_ref().map(|m| m.waypoints.len()).unwrap_or(0);
                let loop_mission = mission_state.mission.as_ref().map(|m| m.loop_mission).unwrap_or(false);
                
                if let Some(mission) = mission_state.mission.as_ref() {
                    if mission_state.current_waypoint >= mission.waypoints.len() {
                        continue;
                    }
                    let wp = &mission.waypoints[mission_state.current_waypoint];
                    let wp_gps = crate::core::types::GpsCoord {
                        latitude: wp.latitude,
                        longitude: wp.longitude,
                        altitude_msl: wp.altitude_agl + terrain.get_height_at_world(
                            kinematics.position.x, kinematics.position.z
                        ) as f64,
                    };
                    let wp_world = geo.gps_to_world(&wp_gps);
                    let delta = wp_world - kinematics.position;
                    let dist = delta.length();

                    if dist < fc.waypoint_acceptance_radius_m {
                        let idx = mission_state.current_waypoint;
                        trace.record_event(SimEvent::WaypointReached {
                            drone_id: identity.id,
                            timestamp: SimTimestamp::now(),
                            waypoint_index: idx,
                        });
                        mission_state.current_waypoint += 1;
                        if mission_state.current_waypoint >= mission_len && loop_mission {
                            mission_state.current_waypoint = 0;
                        }
                    } else {
                        let target_vel = delta.normalize_or_zero() * fc.cruise_speed_ms;
                        let vel_error = target_vel - kinematics.velocity;
                        let accel = vel_error * fc.pid_gains.velocity_p;

                        flight_control.thrust = fc.stabilize_thrust_base + accel.y * fc.pid_gains.altitude_p;
                        flight_control.thrust = flight_control.thrust.clamp(0.3, 0.8);

                        let (_, current_pitch, current_roll) =
                            kinematics.orientation.to_euler(EulerRot::YXZ);

                        flight_control.angular_thrust = Vec3::new(
                            (accel.z * fc.pid_gains.position_p - current_pitch) * fc.pid_gains.attitude_p,
                            0.0,
                            (-accel.x * fc.pid_gains.position_p - current_roll) * fc.pid_gains.attitude_p,
                        );
                    }
                }
            }
            FlightMode::Rtl => {
                // Return to launch (simplified: go to origin)
                let target = Vec3::new(0.0, kinematics.position.y.max(fc.rtl_altitude_m), 0.0);
                let delta = target - kinematics.position;
                let dist = delta.length();

                if dist < fc.waypoint_acceptance_radius_m {
                    flight_control.mode = FlightMode::Land;
                } else {
                    let target_vel = delta.normalize_or_zero() * (fc.cruise_speed_ms * 1.2);
                    let vel_error = target_vel - kinematics.velocity;
                    let accel = vel_error * fc.pid_gains.velocity_p;

                    flight_control.thrust = fc.stabilize_thrust_base + accel.y * fc.pid_gains.altitude_p;
                    flight_control.thrust = flight_control.thrust.clamp(0.3, 0.8);

                    let (_, current_pitch, current_roll) =
                        kinematics.orientation.to_euler(EulerRot::YXZ);

                    flight_control.angular_thrust = Vec3::new(
                        (accel.z * fc.pid_gains.position_p - current_pitch) * fc.pid_gains.attitude_p,
                        0.0,
                        (-accel.x * fc.pid_gains.position_p - current_roll) * fc.pid_gains.attitude_p,
                    );
                }
            }
            FlightMode::Land => {
                // Descend to ground
                let ground = terrain.get_height_at_world(kinematics.position.x, kinematics.position.z);
                let alt_agl = kinematics.position.y - ground;

                if alt_agl < 0.3 {
                    flight_control.thrust = 0.0;
                    kinematics.velocity = Vec3::ZERO;
                    flight_control.angular_thrust = Vec3::ZERO;
                } else {
                    let target_descent = (alt_agl * 0.5).min(fc.land_descent_rate_ms);
                    flight_control.thrust = fc.stabilize_thrust_base - target_descent * fc.pid_gains.altitude_p;
                    flight_control.thrust = flight_control.thrust.clamp(0.2, 0.6);
                    flight_control.angular_thrust = Vec3::new(
                        -kinematics.orientation.to_euler(EulerRot::YXZ).1 * fc.pid_gains.attitude_p,
                        0.0,
                        -kinematics.orientation.to_euler(EulerRot::YXZ).2 * fc.pid_gains.attitude_p,
                    );
                }
            }
        }
    }
}
