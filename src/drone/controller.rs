use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use crate::core::types::{FlightMode, SimEvent, SimTimestamp};
use crate::core::gps::GeoReference;
use crate::drone::{DroneIdentity, Kinematics, FlightControl, MissionState, FleetRegistry};
use crate::drone::types::DroneTypeRegistry;
use crate::eval::trace::TraceCollector;
use crate::world::terrain::TerrainData;

#[derive(Resource, Default)]
pub struct DroneInputState {
    pub forward: f32,
    pub right: f32,
    pub up: f32,
    pub yaw: f32,
    pub mode_request: Option<FlightMode>,
    pub input_captured: bool,
    pub emergency_stop: bool,
}

/// Process keyboard input
pub fn process_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut input_state: ResMut<DroneInputState>,
    mut egui_ctx: EguiContexts,
) {
    let Some(ctx) = egui_ctx.try_ctx_mut() else {
        return;
    };
    if keyboard.just_pressed(KeyCode::Escape) {
        ctx.memory_mut(|mem| mem.request_focus(egui::Id::new("drone_sim_nothing")));
    }
    if ctx.wants_keyboard_input() {
        input_state.input_captured = true;
        input_state.forward = 0.0;
        input_state.right = 0.0;
        input_state.up = 0.0;
        input_state.yaw = 0.0;
        return;
    }
    input_state.input_captured = false;
    input_state.forward = 0.0;
    input_state.right = 0.0;
    input_state.up = 0.0;
    input_state.yaw = 0.0;
    input_state.emergency_stop = false;

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
    if keyboard.just_pressed(KeyCode::KeyX) {
        input_state.emergency_stop = true;
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
    fleet_registry: Res<FleetRegistry>,
) {
    let _dt = time.delta_seconds();

    // Process and clear mode change request
    let mode_request = input_state.mode_request.take();

    for (identity, mut flight_control, mut kinematics, mut mission_state) in query.iter_mut() {
        let is_selected = if fleet_registry.selected().is_empty() {
            true  // No explicit selection means all drones receive input
        } else {
            fleet_registry.is_selected(identity.id)
        };

        // Emergency stop: immediately kill thrust and switch to Land mode
        if is_selected && input_state.emergency_stop {
            let old_mode = flight_control.mode;
            flight_control.mode = FlightMode::Land;
            flight_control.thrust = 0.0;
            flight_control.angular_thrust = Vec3::ZERO;
            flight_control.target_velocity = Vec3::ZERO;
            kinematics.velocity *= 0.5;
            trace.record_event(SimEvent::ModeChanged {
                drone_id: identity.id,
                timestamp: SimTimestamp::now(),
                old_mode,
                new_mode: FlightMode::Land,
            });
        }

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

        // Compute the thrust fraction needed to hover (mass * g / max_thrust)
        let hover_thrust = compute_hover_thrust(spec.airframe.mass_kg, spec.engine.max_thrust_n);

        match flight_control.mode {
            FlightMode::Manual => {
                // Direct throttle control — user sets thrust via keyboard
                if is_selected {
                    // Allow negative thrust for descent (Shift key); clamp to [-0.5, 0.95]
                    flight_control.thrust = input_state.up * fc.manual_thrust_scale + fc.manual_thrust_base;
                    flight_control.thrust = flight_control.thrust.clamp(-0.5, 0.95);
                    flight_control.angular_thrust = Vec3::new(
                        input_state.forward * 0.5,
                        input_state.yaw * 0.5,
                        input_state.right * 0.5,
                    );
                }
            }
            FlightMode::Stabilize => {
                // Self-leveling with throttle around hover point
                if is_selected {
                    flight_control.thrust = input_state.up * fc.manual_thrust_scale + hover_thrust;
                    flight_control.thrust = flight_control.thrust.clamp(0.05, 0.95);
                    let max_tilt = fc.max_tilt_angle_deg.to_radians();
                    let target_pitch = input_state.forward * max_tilt;
                    let target_roll = input_state.right * max_tilt;
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
                // Hold altitude at 10m with PID around hover thrust
                // User can nudge target altitude with Space/Shift for simultaneous vertical control
                let mut target_alt = 10.0f32;
                if is_selected {
                    target_alt += input_state.up * 5.0;
                }
                let alt_error = target_alt - kinematics.position.y;
                flight_control.thrust = hover_thrust + alt_error * fc.pid_gains.altitude_p;
                flight_control.thrust = flight_control.thrust.clamp(0.05, 0.95);

                if is_selected {
                    let max_tilt = fc.max_tilt_angle_deg.to_radians();
                    let target_pitch = input_state.forward * max_tilt;
                    let target_roll = input_state.right * max_tilt;
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
                // Brake to zero velocity while holding altitude
                // User can nudge vertical velocity with Space/Shift for simultaneous control
                let mut target_vel = Vec3::ZERO;
                if is_selected {
                    target_vel.y = input_state.up * fc.max_climb_rate_ms;
                }
                let vel_error = target_vel - kinematics.velocity;
                let accel = vel_error * fc.pid_gains.velocity_p;

                flight_control.thrust = hover_thrust + accel.y * fc.pid_gains.altitude_p;
                flight_control.thrust = flight_control.thrust.clamp(0.05, 0.95);

                let (_, current_pitch, current_roll) =
                    kinematics.orientation.to_euler(EulerRot::YXZ);

                flight_control.angular_thrust = Vec3::new(
                    (accel.z * fc.pid_gains.position_p - current_pitch) * fc.pid_gains.attitude_p,
                    0.0,
                    (-accel.x * fc.pid_gains.position_p - current_roll) * fc.pid_gains.attitude_p,
                );
            }
            FlightMode::Guided => {
                // LLM can command all drones regardless of selection;
                // keyboard input only affects selected drones.
                let has_llm_vel = flight_control.target_velocity.length_squared() > 0.001;
                let target_vel = if has_llm_vel {
                    flight_control.target_velocity
                } else if is_selected {
                    Vec3::new(
                        input_state.right * fc.cruise_speed_ms,
                        input_state.up * fc.max_climb_rate_ms,
                        -input_state.forward * fc.cruise_speed_ms,
                    )
                } else {
                    Vec3::ZERO
                };

                let vel_error = target_vel - kinematics.velocity;
                let accel = vel_error * fc.pid_gains.velocity_p;

                flight_control.thrust = hover_thrust + accel.y * fc.pid_gains.altitude_p;
                flight_control.thrust = flight_control.thrust.clamp(0.05, 0.95);

                let (_, current_pitch, current_roll) =
                    kinematics.orientation.to_euler(EulerRot::YXZ);

                // Preserve LLM yaw command if present; otherwise use keyboard yaw
                let yaw_input = if flight_control.angular_thrust.y.abs() > 0.001 {
                    flight_control.angular_thrust.y
                } else if is_selected {
                    input_state.yaw * 0.5
                } else {
                    0.0
                };

                flight_control.angular_thrust = Vec3::new(
                    (accel.z * fc.pid_gains.position_p - current_pitch) * fc.pid_gains.attitude_p,
                    yaw_input,
                    (-accel.x * fc.pid_gains.position_p - current_roll) * fc.pid_gains.attitude_p,
                );
            }
            FlightMode::Auto => {
                let wp_data = mission_state.mission.as_ref().and_then(|mission| {
                    let mission_len = mission.waypoints.len();
                    if mission_state.current_waypoint >= mission_len {
                        return None;
                    }
                    let wp = &mission.waypoints[mission_state.current_waypoint];
                    Some((wp.latitude, wp.longitude, wp.altitude_agl, mission_len, mission.loop_mission))
                });
                let Some((wp_lat, wp_lon, wp_alt, mission_len, loop_mission)) = wp_data else {
                    if let Some(mission) = mission_state.mission.as_ref() {
                        if mission.loop_mission {
                            mission_state.current_waypoint = 0;
                        } else {
                            flight_control.mode = FlightMode::Loiter;
                        }
                    }
                    continue;
                };
                let wp_gps = crate::core::types::GpsCoord {
                    latitude: wp_lat,
                    longitude: wp_lon,
                    altitude_msl: wp_alt + terrain.get_height_at_world(
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

                    flight_control.thrust = hover_thrust + accel.y * fc.pid_gains.altitude_p;
                    flight_control.thrust = flight_control.thrust.clamp(0.05, 0.95);

                    let (_, current_pitch, current_roll) =
                        kinematics.orientation.to_euler(EulerRot::YXZ);

                    flight_control.angular_thrust = Vec3::new(
                        (accel.z * fc.pid_gains.position_p - current_pitch) * fc.pid_gains.attitude_p,
                        0.0,
                        (-accel.x * fc.pid_gains.position_p - current_roll) * fc.pid_gains.attitude_p,
                    );
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

                    flight_control.thrust = hover_thrust + accel.y * fc.pid_gains.altitude_p;
                    flight_control.thrust = flight_control.thrust.clamp(0.05, 0.95);

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
                // Descend to ground (gravity is applied separately for Land)
                let ground = terrain.get_height_at_world(kinematics.position.x, kinematics.position.z);
                let alt_agl = kinematics.position.y - ground;

                if alt_agl < 0.3 {
                    flight_control.thrust = 0.0;
                    kinematics.velocity = Vec3::ZERO;
                    flight_control.angular_thrust = Vec3::ZERO;
                } else {
                    let target_descent = (alt_agl * 0.5).min(fc.land_descent_rate_ms);
                    flight_control.thrust = hover_thrust - target_descent * fc.pid_gains.altitude_p;
                    flight_control.thrust = flight_control.thrust.clamp(0.05, 0.95);
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

// ===================================================================
// Pure helpers (extracted for testability)
// ===================================================================

/// Compute the thrust fraction [0.05, 0.95] needed to hover.
pub fn compute_hover_thrust(mass_kg: f32, max_thrust_n: f32) -> f32 {
    (mass_kg * 9.81 / max_thrust_n.max(0.01)).clamp(0.05, 0.95)
}

// ===================================================================
// Unit tests
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hover_thrust_exact_balance() {
        // 1 kg drone, 9.81 N max thrust -> needs exactly 1.0 fraction, clamped to 0.95
        assert!((compute_hover_thrust(1.0, 9.81) - 0.95).abs() < 1e-5);
    }

    #[test]
    fn test_hover_thrust_low_mass() {
        // Light drone should still respect minimum thrust
        let thrust = compute_hover_thrust(0.01, 100.0);
        assert_eq!(thrust, 0.05, "Very light drone should clamp to min thrust 0.05");
    }

    #[test]
    fn test_hover_thrust_heavy_drone() {
        // Heavy drone, limited thrust -> clamped to 0.95
        let thrust = compute_hover_thrust(10.0, 10.0);
        assert_eq!(thrust, 0.95, "Under-powered heavy drone should clamp to max thrust 0.95");
    }

    #[test]
    fn test_hover_thrust_mid_range() {
        // 2 kg drone, 40 N max thrust -> 2*9.81/40 = 0.4905
        let thrust = compute_hover_thrust(2.0, 40.0);
        assert!((thrust - 0.4905).abs() < 1e-4);
    }

    #[test]
    fn test_hover_thrust_zero_max_thrust_guard() {
        // Zero max thrust should not panic and should clamp high
        let thrust = compute_hover_thrust(1.0, 0.0);
        assert_eq!(thrust, 0.95, "Zero max thrust should be guarded by max(0.01)");
    }
}
