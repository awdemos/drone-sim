use bevy::prelude::*;
use std::collections::HashMap;

use crate::core::types::{DroneType, MotorType};

#[derive(Debug, Clone)]
pub struct BatteryState {
    pub current_charge_mah: f32,
    pub voltage: f32,
    pub percent: f32,
}

#[derive(Debug, Clone)]
pub struct AirframeSpec {
    pub body_dimensions: Vec3,
    pub arm_length: f32,
    pub arm_count: u8,
    pub arm_thickness: Vec3,
    pub body_color: Color,
    pub arm_color: Color,
    pub drag_coefficient: f32,
    pub quadratic_drag_factor: f32,
    pub mass_kg: f32,
    pub collision_radius: f32,
}

#[derive(Debug, Clone)]
pub struct EngineSpec {
    pub motor_type: MotorType,
    pub kv_rating: u32,
    pub max_thrust_n: f32,
    pub max_torque_nm: f32,
    pub propeller_size_inch: f32,
    pub propeller_blades: u8,
}

#[derive(Debug, Clone)]
pub struct BatterySpec {
    pub capacity_mah: f32,
    pub voltage: f32,
    pub cell_count: u8,
    pub max_flight_time_secs: f32,
}

#[derive(Debug, Clone)]
pub struct CameraSpec {
    pub resolution: (u32, u32),
    pub fov_degrees: f32,
    pub has_gimbal: bool,
    pub sensor_size: &'static str,
}

#[derive(Debug, Clone)]
pub struct PidGains {
    pub attitude_p: f32,
    pub velocity_p: f32,
    pub position_p: f32,
    pub altitude_p: f32,
    pub attitude_i: f32,
    pub velocity_i: f32,
    pub position_i: f32,
    pub altitude_i: f32,
    pub attitude_d: f32,
    pub velocity_d: f32,
    pub position_d: f32,
    pub altitude_d: f32,
    pub integral_limit: f32,
    pub derivative_filter_alpha: f32,
}

#[derive(Debug, Clone)]
pub struct FlightControllerSpec {
    pub max_speed_ms: f32,
    pub max_climb_rate_ms: f32,
    pub max_tilt_angle_deg: f32,
    pub cruise_speed_ms: f32,
    pub waypoint_acceptance_radius_m: f32,
    pub rtl_altitude_m: f32,
    pub land_descent_rate_ms: f32,
    pub manual_thrust_scale: f32,
    pub manual_thrust_base: f32,
    pub stabilize_thrust_base: f32,
    pub pid_gains: PidGains,
}

#[derive(Debug, Clone)]
pub struct DroneTypeSpec {
    pub name: &'static str,
    pub category: &'static str,
    pub airframe: AirframeSpec,
    pub engine: EngineSpec,
    pub battery: BatterySpec,
    pub camera: CameraSpec,
    pub flight_controller: FlightControllerSpec,
}

#[derive(Resource)]
pub struct DroneTypeRegistry {
    pub specs: HashMap<DroneType, DroneTypeSpec>,
}

impl Default for DroneTypeRegistry {
    fn default() -> Self {
        let mut specs = HashMap::new();
        specs.insert(DroneType::TinyWhoop, tiny_whoop_spec());
        specs.insert(DroneType::Racing5Inch, racing_5inch_spec());
        specs.insert(DroneType::Racing7Inch, racing_7inch_spec());
        specs.insert(DroneType::CineWhoop, cinewhoop_spec());
        specs.insert(DroneType::MavicStyle, mavic_spec());
        specs.insert(DroneType::InspireStyle, inspire_spec());
        specs.insert(DroneType::Hexacopter, hexacopter_spec());
        specs.insert(DroneType::Octocopter, octocopter_spec());
        specs.insert(DroneType::VtolFixedWing, vtol_spec());
        specs.insert(DroneType::ToyDrone, toy_drone_spec());
        Self { specs }
    }
}

fn tiny_whoop_spec() -> DroneTypeSpec {
    DroneTypeSpec {
        name: "Tiny Whoop 65mm",
        category: "Racing",
        airframe: AirframeSpec {
            body_dimensions: Vec3::new(0.065, 0.03, 0.065),
            arm_length: 0.025,
            arm_count: 4,
            arm_thickness: Vec3::new(0.008, 0.005, 0.05),
            body_color: Color::srgb(0.9, 0.2, 0.2),
            arm_color: Color::srgb(0.1, 0.1, 0.9),
            drag_coefficient: 0.3,
            quadratic_drag_factor: 0.01,
            mass_kg: 0.020,
            collision_radius: 0.04,
        },
        engine: EngineSpec {
            motor_type: MotorType::Brushless,
            kv_rating: 30000,
            max_thrust_n: 2.0,
            max_torque_nm: 0.1,
            propeller_size_inch: 1.2,
            propeller_blades: 3,
        },
        battery: BatterySpec {
            capacity_mah: 300.0,
            voltage: 4.35,
            cell_count: 1,
            max_flight_time_secs: 300.0,
        },
        camera: CameraSpec {
            resolution: (800, 600),
            fov_degrees: 150.0,
            has_gimbal: false,
            sensor_size: "1/4\"",
        },
        flight_controller: FlightControllerSpec {
            max_speed_ms: 10.0,
            max_climb_rate_ms: 3.0,
            max_tilt_angle_deg: 45.0,
            cruise_speed_ms: 5.0,
            waypoint_acceptance_radius_m: 1.0,
            rtl_altitude_m: 10.0,
            land_descent_rate_ms: 1.0,
            manual_thrust_scale: 0.5,
            manual_thrust_base: 0.35,
            stabilize_thrust_base: 0.45,
            pid_gains: PidGains {
                attitude_p: 3.0,
                velocity_p: 0.8,
                position_p: 0.1,
                altitude_p: 0.08,
                attitude_i: 0.1,
                velocity_i: 0.02,
                position_i: 0.005,
                altitude_i: 0.005,
                attitude_d: 0.3,
                velocity_d: 0.1,
                position_d: 0.02,
                altitude_d: 0.02,
                integral_limit: 5.0,
                derivative_filter_alpha: 0.7,
            },
        },
    }
}

fn racing_5inch_spec() -> DroneTypeSpec {
    DroneTypeSpec {
        name: "Racing 5\"",
        category: "Racing",
        airframe: AirframeSpec {
            body_dimensions: Vec3::new(0.22, 0.06, 0.22),
            arm_length: 0.11,
            arm_count: 4,
            arm_thickness: Vec3::new(0.012, 0.008, 0.22),
            body_color: Color::srgb(0.1, 0.9, 0.1),
            arm_color: Color::srgb(0.1, 0.1, 0.1),
            drag_coefficient: 0.25,
            quadratic_drag_factor: 0.008,
            mass_kg: 0.35,
            collision_radius: 0.15,
        },
        engine: EngineSpec {
            motor_type: MotorType::Brushless,
            kv_rating: 2600,
            max_thrust_n: 60.0,
            max_torque_nm: 2.0,
            propeller_size_inch: 5.0,
            propeller_blades: 3,
        },
        battery: BatterySpec {
            capacity_mah: 1300.0,
            voltage: 14.8,
            cell_count: 4,
            max_flight_time_secs: 360.0,
        },
        camera: CameraSpec {
            resolution: (1280, 720),
            fov_degrees: 150.0,
            has_gimbal: false,
            sensor_size: "1/3\"",
        },
        flight_controller: FlightControllerSpec {
            max_speed_ms: 40.0,
            max_climb_rate_ms: 15.0,
            max_tilt_angle_deg: 60.0,
            cruise_speed_ms: 20.0,
            waypoint_acceptance_radius_m: 2.0,
            rtl_altitude_m: 30.0,
            land_descent_rate_ms: 2.0,
            manual_thrust_scale: 0.6,
            manual_thrust_base: 0.3,
            stabilize_thrust_base: 0.4,
            pid_gains: PidGains {
                attitude_p: 4.0,
                velocity_p: 1.0,
                position_p: 0.15,
                altitude_p: 0.1,
                attitude_i: 0.1,
                velocity_i: 0.02,
                position_i: 0.005,
                altitude_i: 0.005,
                attitude_d: 0.3,
                velocity_d: 0.1,
                position_d: 0.02,
                altitude_d: 0.02,
                integral_limit: 5.0,
                derivative_filter_alpha: 0.7,
            },
        },
    }
}

fn racing_7inch_spec() -> DroneTypeSpec {
    DroneTypeSpec {
        name: "Racing 7\"",
        category: "Racing",
        airframe: AirframeSpec {
            body_dimensions: Vec3::new(0.28, 0.07, 0.28),
            arm_length: 0.14,
            arm_count: 4,
            arm_thickness: Vec3::new(0.014, 0.01, 0.28),
            body_color: Color::srgb(0.9, 0.1, 0.1),
            arm_color: Color::srgb(0.15, 0.15, 0.15),
            drag_coefficient: 0.3,
            quadratic_drag_factor: 0.012,
            mass_kg: 0.60,
            collision_radius: 0.18,
        },
        engine: EngineSpec {
            motor_type: MotorType::Brushless,
            kv_rating: 1900,
            max_thrust_n: 55.0,
            max_torque_nm: 3.0,
            propeller_size_inch: 7.0,
            propeller_blades: 2,
        },
        battery: BatterySpec {
            capacity_mah: 1800.0,
            voltage: 22.2,
            cell_count: 6,
            max_flight_time_secs: 480.0,
        },
        camera: CameraSpec {
            resolution: (1280, 720),
            fov_degrees: 150.0,
            has_gimbal: false,
            sensor_size: "1/3\"",
        },
        flight_controller: FlightControllerSpec {
            max_speed_ms: 35.0,
            max_climb_rate_ms: 12.0,
            max_tilt_angle_deg: 55.0,
            cruise_speed_ms: 18.0,
            waypoint_acceptance_radius_m: 2.0,
            rtl_altitude_m: 30.0,
            land_descent_rate_ms: 2.0,
            manual_thrust_scale: 0.55,
            manual_thrust_base: 0.32,
            stabilize_thrust_base: 0.42,
            pid_gains: PidGains {
                attitude_p: 3.5,
                velocity_p: 0.9,
                position_p: 0.12,
                altitude_p: 0.09,
                attitude_i: 0.1,
                velocity_i: 0.02,
                position_i: 0.005,
                altitude_i: 0.005,
                attitude_d: 0.3,
                velocity_d: 0.1,
                position_d: 0.02,
                altitude_d: 0.02,
                integral_limit: 5.0,
                derivative_filter_alpha: 0.7,
            },
        },
    }
}

fn cinewhoop_spec() -> DroneTypeSpec {
    DroneTypeSpec {
        name: "CineWhoop",
        category: "Cinema",
        airframe: AirframeSpec {
            body_dimensions: Vec3::new(0.15, 0.06, 0.15),
            arm_length: 0.065,
            arm_count: 4,
            arm_thickness: Vec3::new(0.01, 0.008, 0.13),
            body_color: Color::srgb(0.2, 0.2, 0.2),
            arm_color: Color::srgb(0.8, 0.1, 0.1),
            drag_coefficient: 0.4,
            quadratic_drag_factor: 0.015,
            mass_kg: 0.24,
            collision_radius: 0.12,
        },
        engine: EngineSpec {
            motor_type: MotorType::Brushless,
            kv_rating: 4500,
            max_thrust_n: 25.0,
            max_torque_nm: 1.0,
            propeller_size_inch: 3.0,
            propeller_blades: 3,
        },
        battery: BatterySpec {
            capacity_mah: 2150.0,
            voltage: 14.8,
            cell_count: 4,
            max_flight_time_secs: 1380.0,
        },
        camera: CameraSpec {
            resolution: (3840, 2160),
            fov_degrees: 155.0,
            has_gimbal: true,
            sensor_size: "1/2.3\"",
        },
        flight_controller: FlightControllerSpec {
            max_speed_ms: 27.0,
            max_climb_rate_ms: 8.0,
            max_tilt_angle_deg: 35.0,
            cruise_speed_ms: 12.0,
            waypoint_acceptance_radius_m: 1.5,
            rtl_altitude_m: 20.0,
            land_descent_rate_ms: 1.5,
            manual_thrust_scale: 0.45,
            manual_thrust_base: 0.38,
            stabilize_thrust_base: 0.48,
            pid_gains: PidGains {
                attitude_p: 2.5,
                velocity_p: 0.7,
                position_p: 0.08,
                altitude_p: 0.06,
                attitude_i: 0.1,
                velocity_i: 0.02,
                position_i: 0.005,
                altitude_i: 0.005,
                attitude_d: 0.3,
                velocity_d: 0.1,
                position_d: 0.02,
                altitude_d: 0.02,
                integral_limit: 5.0,
                derivative_filter_alpha: 0.7,
            },
        },
    }
}

fn mavic_spec() -> DroneTypeSpec {
    DroneTypeSpec {
        name: "Mavic Style",
        category: "Photography",
        airframe: AirframeSpec {
            body_dimensions: Vec3::new(0.22, 0.08, 0.08),
            arm_length: 0.15,
            arm_count: 4,
            arm_thickness: Vec3::new(0.015, 0.012, 0.30),
            body_color: Color::srgb(0.75, 0.75, 0.78),
            arm_color: Color::srgb(0.65, 0.65, 0.68),
            drag_coefficient: 0.35,
            quadratic_drag_factor: 0.02,
            mass_kg: 0.86,
            collision_radius: 0.20,
        },
        engine: EngineSpec {
            motor_type: MotorType::Brushless,
            kv_rating: 2200,
            max_thrust_n: 45.0,
            max_torque_nm: 2.5,
            propeller_size_inch: 8.8,
            propeller_blades: 3,
        },
        battery: BatterySpec {
            capacity_mah: 5000.0,
            voltage: 15.2,
            cell_count: 4,
            max_flight_time_secs: 2760.0,
        },
        camera: CameraSpec {
            resolution: (5120, 2700),
            fov_degrees: 84.0,
            has_gimbal: true,
            sensor_size: "1\"",
        },
        flight_controller: FlightControllerSpec {
            max_speed_ms: 21.0,
            max_climb_rate_ms: 6.0,
            max_tilt_angle_deg: 35.0,
            cruise_speed_ms: 12.0,
            waypoint_acceptance_radius_m: 2.0,
            rtl_altitude_m: 50.0,
            land_descent_rate_ms: 2.0,
            manual_thrust_scale: 0.4,
            manual_thrust_base: 0.42,
            stabilize_thrust_base: 0.50,
            pid_gains: PidGains {
                attitude_p: 2.0,
                velocity_p: 0.5,
                position_p: 0.05,
                altitude_p: 0.05,
                attitude_i: 0.1,
                velocity_i: 0.02,
                position_i: 0.005,
                altitude_i: 0.005,
                attitude_d: 0.3,
                velocity_d: 0.1,
                position_d: 0.02,
                altitude_d: 0.02,
                integral_limit: 5.0,
                derivative_filter_alpha: 0.7,
            },
        },
    }
}

fn inspire_spec() -> DroneTypeSpec {
    DroneTypeSpec {
        name: "Inspire Style",
        category: "Professional",
        airframe: AirframeSpec {
            body_dimensions: Vec3::new(0.45, 0.15, 0.15),
            arm_length: 0.30,
            arm_count: 4,
            arm_thickness: Vec3::new(0.02, 0.015, 0.60),
            body_color: Color::srgb(0.85, 0.85, 0.88),
            arm_color: Color::srgb(0.75, 0.75, 0.78),
            drag_coefficient: 0.4,
            quadratic_drag_factor: 0.025,
            mass_kg: 3.5,
            collision_radius: 0.35,
        },
        engine: EngineSpec {
            motor_type: MotorType::Brushless,
            kv_rating: 1400,
            max_thrust_n: 120.0,
            max_torque_nm: 8.0,
            propeller_size_inch: 16.7,
            propeller_blades: 3,
        },
        battery: BatterySpec {
            capacity_mah: 4280.0,
            voltage: 22.8,
            cell_count: 6,
            max_flight_time_secs: 1680.0,
        },
        camera: CameraSpec {
            resolution: (7680, 4320),
            fov_degrees: 80.0,
            has_gimbal: true,
            sensor_size: "M4/3",
        },
        flight_controller: FlightControllerSpec {
            max_speed_ms: 26.0,
            max_climb_rate_ms: 8.0,
            max_tilt_angle_deg: 40.0,
            cruise_speed_ms: 15.0,
            waypoint_acceptance_radius_m: 3.0,
            rtl_altitude_m: 60.0,
            land_descent_rate_ms: 2.5,
            manual_thrust_scale: 0.45,
            manual_thrust_base: 0.40,
            stabilize_thrust_base: 0.48,
            pid_gains: PidGains {
                attitude_p: 2.5,
                velocity_p: 0.6,
                position_p: 0.08,
                altitude_p: 0.06,
                attitude_i: 0.1,
                velocity_i: 0.02,
                position_i: 0.005,
                altitude_i: 0.005,
                attitude_d: 0.3,
                velocity_d: 0.1,
                position_d: 0.02,
                altitude_d: 0.02,
                integral_limit: 5.0,
                derivative_filter_alpha: 0.7,
            },
        },
    }
}

fn hexacopter_spec() -> DroneTypeSpec {
    DroneTypeSpec {
        name: "Hexacopter",
        category: "Delivery",
        airframe: AirframeSpec {
            body_dimensions: Vec3::new(0.50, 0.20, 0.50),
            arm_length: 0.40,
            arm_count: 6,
            arm_thickness: Vec3::new(0.025, 0.02, 0.80),
            body_color: Color::srgb(0.2, 0.6, 0.2),
            arm_color: Color::srgb(0.15, 0.15, 0.15),
            drag_coefficient: 0.5,
            quadratic_drag_factor: 0.04,
            mass_kg: 12.0,
            collision_radius: 0.50,
        },
        engine: EngineSpec {
            motor_type: MotorType::Brushless,
            kv_rating: 380,
            max_thrust_n: 400.0,
            max_torque_nm: 25.0,
            propeller_size_inch: 22.0,
            propeller_blades: 2,
        },
        battery: BatterySpec {
            capacity_mah: 16000.0,
            voltage: 44.4,
            cell_count: 12,
            max_flight_time_secs: 1500.0,
        },
        camera: CameraSpec {
            resolution: (1920, 1080),
            fov_degrees: 120.0,
            has_gimbal: true,
            sensor_size: "1/2.3\"",
        },
        flight_controller: FlightControllerSpec {
            max_speed_ms: 15.0,
            max_climb_rate_ms: 5.0,
            max_tilt_angle_deg: 30.0,
            cruise_speed_ms: 8.0,
            waypoint_acceptance_radius_m: 5.0,
            rtl_altitude_m: 80.0,
            land_descent_rate_ms: 1.5,
            manual_thrust_scale: 0.35,
            manual_thrust_base: 0.45,
            stabilize_thrust_base: 0.52,
            pid_gains: PidGains {
                attitude_p: 1.5,
                velocity_p: 0.4,
                position_p: 0.04,
                altitude_p: 0.04,
                attitude_i: 0.1,
                velocity_i: 0.02,
                position_i: 0.005,
                altitude_i: 0.005,
                attitude_d: 0.3,
                velocity_d: 0.1,
                position_d: 0.02,
                altitude_d: 0.02,
                integral_limit: 5.0,
                derivative_filter_alpha: 0.7,
            },
        },
    }
}

fn octocopter_spec() -> DroneTypeSpec {
    DroneTypeSpec {
        name: "Octocopter",
        category: "Heavy Lift",
        airframe: AirframeSpec {
            body_dimensions: Vec3::new(0.80, 0.25, 0.80),
            arm_length: 0.60,
            arm_count: 8,
            arm_thickness: Vec3::new(0.03, 0.025, 1.20),
            body_color: Color::srgb(0.8, 0.5, 0.1),
            arm_color: Color::srgb(0.2, 0.2, 0.2),
            drag_coefficient: 0.6,
            quadratic_drag_factor: 0.06,
            mass_kg: 35.0,
            collision_radius: 0.70,
        },
        engine: EngineSpec {
            motor_type: MotorType::Brushless,
            kv_rating: 240,
            max_thrust_n: 900.0,
            max_torque_nm: 60.0,
            propeller_size_inch: 30.0,
            propeller_blades: 2,
        },
        battery: BatterySpec {
            capacity_mah: 30000.0,
            voltage: 44.4,
            cell_count: 12,
            max_flight_time_secs: 1200.0,
        },
        camera: CameraSpec {
            resolution: (1920, 1080),
            fov_degrees: 100.0,
            has_gimbal: true,
            sensor_size: "1/2.3\"",
        },
        flight_controller: FlightControllerSpec {
            max_speed_ms: 12.0,
            max_climb_rate_ms: 4.0,
            max_tilt_angle_deg: 25.0,
            cruise_speed_ms: 6.0,
            waypoint_acceptance_radius_m: 8.0,
            rtl_altitude_m: 100.0,
            land_descent_rate_ms: 1.0,
            manual_thrust_scale: 0.3,
            manual_thrust_base: 0.48,
            stabilize_thrust_base: 0.55,
            pid_gains: PidGains {
                attitude_p: 1.0,
                velocity_p: 0.3,
                position_p: 0.03,
                altitude_p: 0.03,
                attitude_i: 0.1,
                velocity_i: 0.02,
                position_i: 0.005,
                altitude_i: 0.005,
                attitude_d: 0.3,
                velocity_d: 0.1,
                position_d: 0.02,
                altitude_d: 0.02,
                integral_limit: 5.0,
                derivative_filter_alpha: 0.7,
            },
        },
    }
}

fn vtol_spec() -> DroneTypeSpec {
    DroneTypeSpec {
        name: "VTOL Fixed Wing",
        category: "Survey",
        airframe: AirframeSpec {
            body_dimensions: Vec3::new(1.2, 0.25, 0.30),
            arm_length: 0.45,
            arm_count: 4,
            arm_thickness: Vec3::new(0.025, 0.02, 0.90),
            body_color: Color::srgb(0.9, 0.9, 0.92),
            arm_color: Color::srgb(0.7, 0.7, 0.72),
            drag_coefficient: 0.25,
            quadratic_drag_factor: 0.02,
            mass_kg: 25.0,
            collision_radius: 0.80,
        },
        engine: EngineSpec {
            motor_type: MotorType::Brushless,
            kv_rating: 380,
            max_thrust_n: 200.0,
            max_torque_nm: 15.0,
            propeller_size_inch: 16.0,
            propeller_blades: 2,
        },
        battery: BatterySpec {
            capacity_mah: 22000.0,
            voltage: 44.4,
            cell_count: 12,
            max_flight_time_secs: 14400.0,
        },
        camera: CameraSpec {
            resolution: (1920, 1080),
            fov_degrees: 100.0,
            has_gimbal: true,
            sensor_size: "1/2.3\"",
        },
        flight_controller: FlightControllerSpec {
            max_speed_ms: 30.0,
            max_climb_rate_ms: 8.0,
            max_tilt_angle_deg: 45.0,
            cruise_speed_ms: 20.0,
            waypoint_acceptance_radius_m: 10.0,
            rtl_altitude_m: 120.0,
            land_descent_rate_ms: 2.0,
            manual_thrust_scale: 0.4,
            manual_thrust_base: 0.42,
            stabilize_thrust_base: 0.50,
            pid_gains: PidGains {
                attitude_p: 2.0,
                velocity_p: 0.5,
                position_p: 0.06,
                altitude_p: 0.05,
                attitude_i: 0.1,
                velocity_i: 0.02,
                position_i: 0.005,
                altitude_i: 0.005,
                attitude_d: 0.3,
                velocity_d: 0.1,
                position_d: 0.02,
                altitude_d: 0.02,
                integral_limit: 5.0,
                derivative_filter_alpha: 0.7,
            },
        },
    }
}

fn toy_drone_spec() -> DroneTypeSpec {
    DroneTypeSpec {
        name: "Toy Drone",
        category: "Toy",
        airframe: AirframeSpec {
            body_dimensions: Vec3::new(0.08, 0.04, 0.08),
            arm_length: 0.04,
            arm_count: 4,
            arm_thickness: Vec3::new(0.008, 0.005, 0.08),
            body_color: Color::srgb(0.9, 0.6, 0.1),
            arm_color: Color::srgb(0.1, 0.6, 0.9),
            drag_coefficient: 0.4,
            quadratic_drag_factor: 0.02,
            mass_kg: 0.027,
            collision_radius: 0.05,
        },
        engine: EngineSpec {
            motor_type: MotorType::Brushed,
            kv_rating: 15000,
            max_thrust_n: 2.0,
            max_torque_nm: 0.08,
            propeller_size_inch: 1.2,
            propeller_blades: 4,
        },
        battery: BatterySpec {
            capacity_mah: 300.0,
            voltage: 3.7,
            cell_count: 1,
            max_flight_time_secs: 270.0,
        },
        camera: CameraSpec {
            resolution: (800, 600),
            fov_degrees: 150.0,
            has_gimbal: false,
            sensor_size: "1/5\"",
        },
        flight_controller: FlightControllerSpec {
            max_speed_ms: 6.0,
            max_climb_rate_ms: 2.0,
            max_tilt_angle_deg: 30.0,
            cruise_speed_ms: 3.0,
            waypoint_acceptance_radius_m: 1.0,
            rtl_altitude_m: 8.0,
            land_descent_rate_ms: 0.8,
            manual_thrust_scale: 0.4,
            manual_thrust_base: 0.40,
            stabilize_thrust_base: 0.50,
            pid_gains: PidGains {
                attitude_p: 2.0,
                velocity_p: 0.5,
                position_p: 0.06,
                altitude_p: 0.05,
                attitude_i: 0.1,
                velocity_i: 0.02,
                position_i: 0.005,
                altitude_i: 0.005,
                attitude_d: 0.3,
                velocity_d: 0.1,
                position_d: 0.02,
                altitude_d: 0.02,
                integral_limit: 5.0,
                derivative_filter_alpha: 0.7,
            },
        },
    }
}
