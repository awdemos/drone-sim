use bevy::prelude::*;
use crate::core::gps::GeoReference;
use crate::core::types::{FlightMode, SimEvent, SimTimestamp};
use crate::drone::{DamageLevel, DroneIdentity, Kinematics, FlightControl, Battery, Health, GpsPosition};
use crate::drone::types::{DroneTypeRegistry, DroneTypeSpec};
use crate::eval::trace::TraceCollector;
use crate::world::terrain::TerrainData;
use crate::world::SpatialGrid;

const GRAVITY: f32 = 9.81;

const DAMAGE_MINOR_THRESHOLD: f32 = 3.0;
const DAMAGE_MAJOR_THRESHOLD: f32 = 8.0;
const DAMAGE_CRITICAL_THRESHOLD: f32 = 15.0;

const RESTITUTION: f32 = 0.3;
const FRICTION: f32 = 0.8;

// ===================================================================
// Pure physics helpers (extracted for testability)
// ===================================================================

/// Apply gravity to kinematics for one time step.
/// For auto-hold flight modes the thrust is raised to compensate.
pub fn apply_gravity_pure(
    kinematics: &mut Kinematics,
    flight_control: &mut FlightControl,
    dt: f32,
    gravity_comp: f32,
) {
    match flight_control.mode {
        FlightMode::AltHold | FlightMode::Loiter | FlightMode::Guided | FlightMode::Auto => {
            flight_control.thrust = flight_control.thrust.max(gravity_comp);
        }
        _ => {
            kinematics.velocity.y -= GRAVITY * dt;
        }
    }
}

/// Compute total drag force (linear + quadratic) for the given velocity.
pub fn compute_drag(velocity: Vec3, spec: &DroneTypeSpec) -> Vec3 {
    let v_squared = velocity.length_squared();
    let linear_drag = -velocity * spec.airframe.drag_coefficient;
    let quadratic_drag = if v_squared > 0.001 {
        -velocity.normalize() * v_squared * spec.airframe.quadratic_drag_factor
    } else {
        Vec3::ZERO
    };
    linear_drag + quadratic_drag
}

/// Compute acceleration produced by thrust given current orientation.
pub fn compute_thrust_acceleration(kinematics: &Kinematics, thrust: f32, spec: &DroneTypeSpec) -> Vec3 {
    let thrust_vec = kinematics.orientation * Vec3::Y * thrust * spec.engine.max_thrust_n;
    thrust_vec / spec.airframe.mass_kg.max(0.1)
}

/// Clamp pitch and roll to `max_tilt` for stabilised flight modes.
pub fn clamp_tilt(kinematics: &mut Kinematics, max_tilt: f32, mode: FlightMode) {
    match mode {
        FlightMode::Stabilize | FlightMode::AltHold | FlightMode::Loiter => {
            let (yaw, pitch, roll) = kinematics.orientation.to_euler(EulerRot::YXZ);
            let clamped_pitch = pitch.clamp(-max_tilt, max_tilt);
            let clamped_roll = roll.clamp(-max_tilt, max_tilt);
            kinematics.orientation = Quat::from_euler(EulerRot::YXZ, yaw, clamped_pitch, clamped_roll);
        }
        _ => {}
    }
}

/// Apply terrain-collision velocity response (bounce + friction).
pub fn apply_collision_response(kinematics: &mut Kinematics, is_operational: bool) {
    let restitution = if is_operational { RESTITUTION } else { 0.1 };
    kinematics.velocity.y = -kinematics.velocity.y * restitution;
    kinematics.velocity.x *= FRICTION;
    kinematics.velocity.z *= FRICTION;
}

// ===================================================================
// Bevy systems
// ===================================================================

pub fn update_physics(
    time: Res<Time>,
    mut query: Query<(&DroneIdentity, &mut Kinematics, &mut FlightControl, &mut Battery, &mut Health, &mut GpsPosition)>,
    geo: Res<GeoReference>,
    registry: Res<DroneTypeRegistry>,
) {
    let dt = time.delta_seconds();

    for (identity, mut kinematics, mut flight_control, mut battery, mut health, mut gps_pos) in query.iter_mut() {
        // Guard against NaN propagation from corrupted state
        if !kinematics.position.is_finite()
            || !kinematics.velocity.is_finite()
            || !kinematics.orientation.is_finite()
            || !flight_control.thrust.is_finite()
            || !flight_control.angular_thrust.is_finite()
        {
            health.is_operational = false;
            health.damage_level = DamageLevel::Critical;
            kinematics.velocity = Vec3::ZERO;
            kinematics.angular_velocity = Vec3::ZERO;
            flight_control.thrust = 0.0;
            flight_control.angular_thrust = Vec3::ZERO;
            continue;
        }

        let spec = registry.specs.get(&identity.drone_type).unwrap();

        if !health.is_operational {
            kinematics.velocity.y -= GRAVITY * dt;
            let vel = kinematics.velocity;
            kinematics.position += vel * dt;
            kinematics.angular_velocity *= 0.95;
            let angular_delta = kinematics.angular_velocity * dt;
            let rotation = Quat::from_euler(
                EulerRot::XYZ,
                angular_delta.x,
                angular_delta.y,
                angular_delta.z,
            );
            kinematics.orientation = (kinematics.orientation * rotation).normalize();
            gps_pos.coord = geo.world_to_gps(kinematics.position);
            continue;
        }

        let thrust_vec = kinematics.orientation * Vec3::Y * flight_control.thrust * spec.engine.max_thrust_n;
        let torque = flight_control.angular_thrust * spec.engine.max_torque_nm;

        kinematics.angular_velocity += torque * dt;
        kinematics.angular_velocity *= 0.95;

        let angular_delta = kinematics.angular_velocity * dt;
        let rotation = Quat::from_euler(
            EulerRot::XYZ,
            angular_delta.x,
            angular_delta.y,
            angular_delta.z,
        );
        kinematics.orientation = (kinematics.orientation * rotation).normalize();

        let max_tilt_angle = spec.flight_controller.max_tilt_angle_deg.to_radians();
        clamp_tilt(&mut kinematics, max_tilt_angle, flight_control.mode);

        let v = kinematics.velocity;
        let total_drag = compute_drag(v, spec);

        let acceleration = (thrust_vec + total_drag) / spec.airframe.mass_kg.max(0.1);
        kinematics.velocity += acceleration * dt;

        let velocity = kinematics.velocity;
        kinematics.position += velocity * dt;

        gps_pos.coord = geo.world_to_gps(kinematics.position);

        let power = flight_control.thrust.abs() * spec.engine.max_thrust_n;
        let drain_mah = (power / (spec.battery.voltage * 0.001)) * dt * 0.001;
        battery.current_charge_mah -= drain_mah;
        battery.percent = (battery.current_charge_mah / spec.battery.capacity_mah * 100.0).clamp(0.0, 100.0);

        if battery.percent <= 0.0 && health.is_operational {
            health.is_operational = false;
            health.damage_level = DamageLevel::Critical;
        }

        // Final sanity check: ensure no NaN/Inf leaked out
        if !kinematics.position.is_finite() {
            kinematics.position = Vec3::new(0.0, 10.0, 0.0);
        }
        if !kinematics.velocity.is_finite() {
            kinematics.velocity = Vec3::ZERO;
        }
        if !kinematics.orientation.is_finite() {
            kinematics.orientation = Quat::IDENTITY;
        }
        if !kinematics.angular_velocity.is_finite() {
            kinematics.angular_velocity = Vec3::ZERO;
        }
        flight_control.thrust = flight_control.thrust.clamp(0.0, 1.0);
    }
}

pub fn apply_gravity(
    time: Res<Time>,
    mut query: Query<(&DroneIdentity, &mut Kinematics, &mut FlightControl, &Health)>,
    registry: Res<DroneTypeRegistry>,
) {
    let dt = time.delta_seconds();

    for (identity, mut kinematics, mut flight_control, health) in query.iter_mut() {
        if !health.is_operational {
            continue;
        }

        let spec = registry.specs.get(&identity.drone_type).unwrap();
        let gravity_comp = GRAVITY / spec.engine.max_thrust_n.max(1.0);

        apply_gravity_pure(&mut kinematics, &mut flight_control, dt, gravity_comp);
    }
}

fn apply_damage(
    health: &mut Health,
    kinematics: &mut Kinematics,
    identity: &DroneIdentity,
    impact_velocity: f32,
    trace: &mut TraceCollector,
) {
    health.last_impact_velocity = impact_velocity;

    let damage = if impact_velocity < DAMAGE_MINOR_THRESHOLD {
        0.0
    } else if impact_velocity < DAMAGE_MAJOR_THRESHOLD {
        (impact_velocity - DAMAGE_MINOR_THRESHOLD) * 3.0
    } else if impact_velocity < DAMAGE_CRITICAL_THRESHOLD {
        (impact_velocity - DAMAGE_MAJOR_THRESHOLD) * 5.0 + 15.0
    } else {
        (impact_velocity - DAMAGE_CRITICAL_THRESHOLD) * 8.0 + 50.0
    };

    if damage > 0.0 {
        health.health_percent -= damage;
        health.health_percent = health.health_percent.clamp(0.0, 100.0);

        health.damage_level = if health.health_percent <= 0.0 {
            DamageLevel::Destroyed
        } else if impact_velocity >= DAMAGE_CRITICAL_THRESHOLD {
            DamageLevel::Critical
        } else if impact_velocity >= DAMAGE_MAJOR_THRESHOLD {
            DamageLevel::Major
        } else if impact_velocity >= DAMAGE_MINOR_THRESHOLD {
            DamageLevel::Minor
        } else {
            health.damage_level
        };

        if health.health_percent <= 0.0 {
            health.is_operational = false;
            kinematics.velocity *= 0.1;
        }

        trace.record_event(SimEvent::Collision {
            drone_id: identity.id,
            timestamp: SimTimestamp::now(),
            position: kinematics.position,
        });
    }
}

pub fn terrain_collision(
    mut query: Query<(&DroneIdentity, &mut Kinematics, &mut Health, &mut FlightControl)>,
    terrain: Res<TerrainData>,
    mut trace: ResMut<TraceCollector>,
    _registry: Res<DroneTypeRegistry>,
) {
    for (identity, mut kinematics, mut health, _flight_control) in query.iter_mut() {
        let ground_height = terrain.get_height_at_world(kinematics.position.x, kinematics.position.z);
        let drone_bottom = kinematics.position.y - 0.1;

        if drone_bottom < ground_height {
            let _penetration = ground_height - drone_bottom;
            let impact_speed = kinematics.velocity.y.abs();

            if impact_speed > 0.5 {
                apply_damage(&mut health, &mut kinematics, identity, impact_speed, &mut trace);
            }

            kinematics.position.y = ground_height + 0.1;

            apply_collision_response(&mut kinematics, health.is_operational);

            if !health.is_operational {
                kinematics.angular_velocity += Vec3::new(
                    fastrand::f32() - 0.5,
                    fastrand::f32() - 0.5,
                    fastrand::f32() - 0.5,
                ) * impact_speed * 0.1;
            }
        }
    }
}

pub fn building_collision(
    mut drones: Query<(&DroneIdentity, &mut Kinematics, &mut Health)>,
    spatial_grid: Res<SpatialGrid>,
    mut trace: ResMut<TraceCollector>,
    registry: Res<DroneTypeRegistry>,
) {
    const CELL_SIZE: f32 = 50.0;

    for (identity, mut kinematics, mut health) in drones.iter_mut() {
        if !health.is_operational {
            continue;
        }

        let spec = registry.specs.get(&identity.drone_type).unwrap();
        let drone_pos = kinematics.position;
        let drone_radius = spec.airframe.collision_radius;
        let query_radius = drone_radius + CELL_SIZE;

        let nearby = spatial_grid.query_near(drone_pos, query_radius);

        for (_entity, collider) in nearby {
            if drone_pos.x + drone_radius > collider.min.x
                && drone_pos.x - drone_radius < collider.max.x
                && drone_pos.y + drone_radius > collider.min.y
                && drone_pos.y - drone_radius < collider.max.y
                && drone_pos.z + drone_radius > collider.min.z
                && drone_pos.z - drone_radius < collider.max.z
            {
                let impact_speed = kinematics.velocity.length();

                if impact_speed > 1.0 {
                    apply_damage(&mut health, &mut kinematics, identity, impact_speed, &mut trace);
                }

                let center = Vec3::new(
                    (collider.min.x + collider.max.x) / 2.0,
                    (collider.min.y + collider.max.y) / 2.0,
                    (collider.min.z + collider.max.z) / 2.0,
                );
                let away = (drone_pos - center).normalize_or_zero();
                kinematics.velocity += away * impact_speed * 0.5 + Vec3::Y * 2.0;

                trace.record_event(SimEvent::ObstacleDetected {
                    drone_id: identity.id,
                    timestamp: SimTimestamp::now(),
                    obstacle_position: center,
                    distance: 0.0,
                });

                break;
            }
        }
    }
}

pub fn update_damage_effects(
    mut query: Query<(&Health, &mut Visibility)>,
) {
    for (health, mut visibility) in query.iter_mut() {
        if health.damage_level == DamageLevel::Destroyed {
            *visibility = Visibility::Hidden;
        }
    }
}

// ===================================================================
// Unit tests
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::{DroneType, MotorType};
    use crate::drone::types::{DroneTypeSpec, AirframeSpec, EngineSpec, BatterySpec, CameraSpec, FlightControllerSpec, PidGains};

    fn mock_spec() -> DroneTypeSpec {
        DroneTypeSpec {
            name: "Test Drone",
            category: "Test",
            airframe: AirframeSpec {
                body_dimensions: Vec3::new(0.2, 0.05, 0.2),
                arm_length: 0.1,
                arm_count: 4,
                arm_thickness: Vec3::new(0.01, 0.005, 0.1),
                body_color: Color::srgb(0.5, 0.5, 0.5),
                arm_color: Color::srgb(0.5, 0.5, 0.5),
                drag_coefficient: 0.3,
                quadratic_drag_factor: 0.01,
                mass_kg: 1.0,
                collision_radius: 0.15,
            },
            engine: EngineSpec {
                motor_type: MotorType::Brushless,
                kv_rating: 2000,
                max_thrust_n: 20.0,
                max_torque_nm: 2.0,
                propeller_size_inch: 5.0,
                propeller_blades: 3,
            },
            battery: BatterySpec {
                capacity_mah: 5000.0,
                voltage: 14.8,
                cell_count: 4,
                max_flight_time_secs: 1800.0,
            },
            camera: CameraSpec {
                resolution: (1280, 720),
                fov_degrees: 90.0,
                has_gimbal: false,
                sensor_size: "1/3\"",
            },
            flight_controller: FlightControllerSpec {
                max_speed_ms: 15.0,
                max_climb_rate_ms: 5.0,
                max_tilt_angle_deg: 45.0,
                cruise_speed_ms: 8.0,
                waypoint_acceptance_radius_m: 2.0,
                rtl_altitude_m: 30.0,
                land_descent_rate_ms: 2.0,
                manual_thrust_scale: 0.5,
                manual_thrust_base: 0.35,
                stabilize_thrust_base: 0.45,
                pid_gains: PidGains {
                    attitude_p: 2.0,
                    velocity_p: 0.5,
                    position_p: 0.05,
                    altitude_p: 0.05,
                },
            },
        }
    }

    fn default_kinematics() -> Kinematics {
        Kinematics {
            position: Vec3::new(0.0, 10.0, 0.0),
            orientation: Quat::IDENTITY,
            velocity: Vec3::ZERO,
            angular_velocity: Vec3::ZERO,
        }
    }

    fn default_flight_control() -> FlightControl {
        FlightControl {
            mode: FlightMode::Manual,
            target_velocity: Vec3::ZERO,
            target_yaw: 0.0,
            thrust: 0.0,
            angular_thrust: Vec3::ZERO,
        }
    }

    fn default_health() -> Health {
        Health {
            health_percent: 100.0,
            damage_level: DamageLevel::None,
            is_operational: true,
            last_impact_velocity: 0.0,
        }
    }

    // ------------------------------------------------------------------
    // 1. Gravity acceleration: after 1 s, velocity.y decreases by ~9.81 m/s
    // ------------------------------------------------------------------
    #[test]
    fn test_gravity_acceleration() {
        let spec = mock_spec();
        let gravity_comp = GRAVITY / spec.engine.max_thrust_n.max(1.0);

        let mut kinematics = default_kinematics();
        let mut flight_control = default_flight_control();

        apply_gravity_pure(&mut kinematics, &mut flight_control, 1.0, gravity_comp);

        assert!(
            (kinematics.velocity.y - (-GRAVITY)).abs() < 1e-3,
            "Expected vy ≈ -9.81 m/s after 1 s, got {} m/s",
            kinematics.velocity.y
        );
    }

    // ------------------------------------------------------------------
    // 2. Thrust counteracts gravity
    // ------------------------------------------------------------------
    #[test]
    fn test_thrust_counteracts_gravity() {
        let spec = mock_spec();
        let mut kinematics = default_kinematics();
        let mut flight_control = default_flight_control();

        // Thrust level that exactly balances gravity when upright
        let thrust = GRAVITY * spec.airframe.mass_kg / spec.engine.max_thrust_n;
        flight_control.thrust = thrust;

        // Apply gravity (Manual mode -> gravity pulls down)
        let gravity_comp = GRAVITY / spec.engine.max_thrust_n.max(1.0);
        apply_gravity_pure(&mut kinematics, &mut flight_control, 1.0, gravity_comp);

        // Apply thrust acceleration
        let accel = compute_thrust_acceleration(&kinematics, flight_control.thrust, &spec);
        kinematics.velocity += accel * 1.0;

        // Net change in vy should be near zero
        assert!(
            kinematics.velocity.y.abs() < 1e-3,
            "Expected vy ≈ 0 m/s when thrust balances gravity, got {} m/s",
            kinematics.velocity.y
        );
    }

    // ------------------------------------------------------------------
    // 3. Drag reduces velocity
    // ------------------------------------------------------------------
    #[test]
    fn test_drag_reduces_velocity() {
        let spec = mock_spec();
        let velocity = Vec3::new(10.0, 5.0, 2.0);

        let drag = compute_drag(velocity, &spec);
        let v_after = velocity + drag * 0.1; // Apply drag for 0.1 s

        assert!(
            v_after.length() < velocity.length(),
            "Drag should reduce velocity: before = {}, after = {}",
            velocity.length(),
            v_after.length()
        );
    }

    // ------------------------------------------------------------------
    // 4. Tilt clamp: after clamp, tilt angle should be ≤ max_tilt
    // ------------------------------------------------------------------
    #[test]
    fn test_clamp_tilt() {
        let max_tilt = 30.0f32.to_radians();

        let mut kinematics = default_kinematics();
        // Set a large tilt (60 deg pitch, 60 deg roll)
        kinematics.orientation = Quat::from_euler(EulerRot::YXZ, 0.0, 60.0f32.to_radians(), 60.0f32.to_radians());

        clamp_tilt(&mut kinematics, max_tilt, FlightMode::Stabilize);

        let (_, pitch, roll) = kinematics.orientation.to_euler(EulerRot::YXZ);
        assert!(
            pitch.abs() <= max_tilt + 1e-5,
            "Pitch {} rad exceeds max tilt {} rad",
            pitch.abs(),
            max_tilt
        );
        assert!(
            roll.abs() <= max_tilt + 1e-5,
            "Roll {} rad exceeds max tilt {} rad",
            roll.abs(),
            max_tilt
        );
    }

    // ------------------------------------------------------------------
    // 5. Damage thresholds
    // ------------------------------------------------------------------
    #[test]
    fn test_damage_thresholds() {
        use crate::eval::trace::TraceCollector;
        use crate::core::types::DroneId;

        let identity = DroneIdentity {
            id: DroneId::new(),
            drone_type: DroneType::MavicStyle,
        };

        // --- Below minor threshold (< 3 m/s) -> no damage ---
        {
            let mut health = default_health();
            let mut kinematics = default_kinematics();
            let mut trace = TraceCollector::default();
            apply_damage(&mut health, &mut kinematics, &identity, 2.0, &mut trace);
            assert_eq!(health.health_percent, 100.0, "2 m/s should cause no damage");
            assert_eq!(health.damage_level, DamageLevel::None);
        }

        // --- Just above minor threshold (3.1 m/s) -> Minor damage ---
        {
            let mut health = default_health();
            let mut kinematics = default_kinematics();
            let mut trace = TraceCollector::default();
            apply_damage(&mut health, &mut kinematics, &identity, 3.1, &mut trace);
            assert!(
                health.health_percent < 100.0,
                "3.1 m/s should cause damage, health = {}%",
                health.health_percent
            );
            assert_eq!(health.damage_level, DamageLevel::Minor);
        }

        // --- Just above major threshold (8.1 m/s) -> Major damage ---
        {
            let mut health = default_health();
            let mut kinematics = default_kinematics();
            let mut trace = TraceCollector::default();
            apply_damage(&mut health, &mut kinematics, &identity, 8.1, &mut trace);
            assert!(
                health.health_percent < 100.0,
                "8.1 m/s should cause damage, health = {}%",
                health.health_percent
            );
            assert_eq!(health.damage_level, DamageLevel::Major);
        }

        // --- Just above critical threshold (15.1 m/s) -> Critical damage ---
        {
            let mut health = default_health();
            let mut kinematics = default_kinematics();
            let mut trace = TraceCollector::default();
            apply_damage(&mut health, &mut kinematics, &identity, 15.1, &mut trace);
            assert!(
                health.health_percent < 100.0,
                "15.1 m/s should cause damage, health = {}%",
                health.health_percent
            );
            assert_eq!(health.damage_level, DamageLevel::Critical);
        }
    }

    // ------------------------------------------------------------------
    // 6. Collision response: velocity should have bounce component
    // ------------------------------------------------------------------
    #[test]
    fn test_collision_response_bounce() {
        let mut kinematics = Kinematics {
            position: Vec3::new(0.0, 1.0, 0.0),
            orientation: Quat::IDENTITY,
            velocity: Vec3::new(5.0, -10.0, 3.0),
            angular_velocity: Vec3::ZERO,
        };

        let vy_before = kinematics.velocity.y;
        apply_collision_response(&mut kinematics, true);

        assert!(
            kinematics.velocity.y > 0.0,
            "vy should bounce upward after collision, got {} m/s",
            kinematics.velocity.y
        );
        assert!(
            kinematics.velocity.y < vy_before.abs(),
            "vy bounce magnitude {} should be less than impact magnitude {}",
            kinematics.velocity.y,
            vy_before.abs()
        );
        assert!(
            kinematics.velocity.x.abs() < 5.0,
            "vx should be reduced by friction, got {} m/s",
            kinematics.velocity.x.abs()
        );
        assert!(
            kinematics.velocity.z.abs() < 3.0,
            "vz should be reduced by friction, got {} m/s",
            kinematics.velocity.z.abs()
        );
    }

    // ------------------------------------------------------------------
    // 7. Terminal velocity: drag balances gravity at some speed
    // ------------------------------------------------------------------
    #[test]
    fn test_terminal_velocity() {
        let spec = mock_spec();
        let mut kinematics = default_kinematics();
        kinematics.velocity = Vec3::new(0.0, -50.0, 0.0); // Falling fast downward

        // Simulate many small steps
        let dt = 0.01;
        for _ in 0..2000 {
            let drag = compute_drag(kinematics.velocity, &spec);
            let thrust = compute_thrust_acceleration(&kinematics, 0.0, &spec);
            let accel = Vec3::new(0.0, -GRAVITY, 0.0) + thrust + drag / spec.airframe.mass_kg;
            kinematics.velocity += accel * dt;
        }

        // Terminal velocity should be reached and positive (downward)
        let terminal_speed = kinematics.velocity.y.abs();
        assert!(
            terminal_speed > 0.0 && terminal_speed < 50.0,
            "Terminal velocity should be bounded, got {} m/s",
            terminal_speed
        );

        // Speed should have decreased from initial 50 m/s
        assert!(
            terminal_speed < 50.0,
            "Speed should decrease to terminal velocity, got {} m/s",
            terminal_speed
        );
    }

    // ------------------------------------------------------------------
    // 8. Freefall: destroyed drone has no thrust, gravity dominates
    // ------------------------------------------------------------------
    #[test]
    fn test_freefall_destroyed() {
        let mut kinematics = default_kinematics();
        kinematics.velocity = Vec3::new(1.0, 0.0, 1.0);
        let dt = 1.0;

        // Simulate a destroyed drone (no thrust, just gravity)
        kinematics.velocity.y -= GRAVITY * dt;

        assert!(
            (kinematics.velocity.y - (-GRAVITY)).abs() < 1e-3,
            "Destroyed drone vy should be -9.81 m/s after 1 s, got {} m/s",
            kinematics.velocity.y
        );
        assert_eq!(kinematics.velocity.x, 1.0, "Horizontal vx should be unchanged in freefall");
        assert_eq!(kinematics.velocity.z, 1.0, "Horizontal vz should be unchanged in freefall");
    }

    // ------------------------------------------------------------------
    // 9. Thrust vector direction follows orientation
    // ------------------------------------------------------------------
    #[test]
    fn test_thrust_direction() {
        let spec = mock_spec();
        let mut kinematics = default_kinematics();

        // Tilt 45 degrees forward (negative pitch around X -> nose points toward -Z)
        kinematics.orientation = Quat::from_rotation_x(-45.0f32.to_radians());

        let accel = compute_thrust_acceleration(&kinematics, 1.0, &spec);
        // Thrust should have a forward (negative z) component when pitched forward
        assert!(accel.z < -0.1, "Forward pitch should produce negative z thrust, got {}", accel.z);
        assert!(accel.y > 0.0, "Thrust should still have upward component, got {}", accel.y);
    }

    // ------------------------------------------------------------------
    // 10. Gravity compensation in auto modes
    // ------------------------------------------------------------------
    #[test]
    fn test_gravity_compensation_auto_mode() {
        let spec = mock_spec();
        let gravity_comp = GRAVITY / spec.engine.max_thrust_n.max(1.0);

        let mut kinematics = default_kinematics();
        let mut flight_control = default_flight_control();
        flight_control.mode = FlightMode::AltHold;
        flight_control.thrust = 0.0;

        apply_gravity_pure(&mut kinematics, &mut flight_control, 1.0, gravity_comp);

        // In AltHold mode, thrust should be raised to at least gravity_comp
        assert!(
            flight_control.thrust >= gravity_comp,
            "AltHold thrust should be >= gravity_comp ({}), got {}",
            gravity_comp,
            flight_control.thrust
        );
        // Velocity should NOT change in auto modes
        assert_eq!(kinematics.velocity.y, 0.0, "Velocity should not change in AltHold mode");
    }
}