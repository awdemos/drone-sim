use bevy::prelude::*;
use crate::drone::{Kinematics, GpsPosition, DroneIdentity};
use crate::core::gps::GeoReference;

#[derive(Component)]
pub struct GpsNoise {
    pub enabled: bool,
    pub noise_stddev_m: f32,
    pub drift_rate_mps: f32,
    pub drift_current: Vec3,
    pub drift_direction: Vec3,
    pub drift_timer: f32,
    pub drift_change_interval: f32,
    pub multipath_enabled: bool,
    pub multipath_threshold_alt: f32,
    pub last_noisy_position: Vec3,
    pub fix_lost: bool,
    pub fix_lost_timer: f32,
    pub fix_lost_duration: f32,
    pub hdop: f32,
    pub vdop: f32,
    pub num_satellites: u8,
}

impl Default for GpsNoise {
    fn default() -> Self {
        Self {
            enabled: true,
            noise_stddev_m: 2.5,
            drift_rate_mps: 0.02,
            drift_current: Vec3::ZERO,
            drift_direction: Vec3::new(
                (fastrand::f32() - 0.5) * 2.0,
                0.0,
                (fastrand::f32() - 0.5) * 2.0,
            ).normalize(),
            drift_timer: 0.0,
            drift_change_interval: 30.0,
            multipath_enabled: true,
            multipath_threshold_alt: 15.0,
            last_noisy_position: Vec3::ZERO,
            fix_lost: false,
            fix_lost_timer: 0.0,
            fix_lost_duration: 0.0,
            hdop: 1.0,
            vdop: 1.5,
            num_satellites: 12,
        }
    }
}

#[derive(Component, Default)]
pub struct SensorNoise {
    pub barometer_alt_bias: f32,
    pub barometer_noise_stddev: f32,
    pub magnetometer_bias: Vec3,
    pub magnetometer_noise_stddev: f32,
    pub imu_accel_bias: Vec3,
    pub imu_accel_noise_stddev: f32,
    pub imu_gyro_bias: Vec3,
    pub imu_gyro_noise_stddev: f32,
    pub last_baro_altitude: f32,
    pub last_mag_heading: f32,
    pub last_accel: Vec3,
    pub last_gyro: Vec3,
}

impl SensorNoise {
    pub fn realistic() -> Self {
        Self {
            barometer_alt_bias: 0.5,
            barometer_noise_stddev: 0.3,
            magnetometer_bias: Vec3::new(0.01, 0.02, -0.01),
            magnetometer_noise_stddev: 0.005,
            imu_accel_bias: Vec3::new(0.02, -0.01, 0.015),
            imu_accel_noise_stddev: 0.08,
            imu_gyro_bias: Vec3::new(0.001, -0.002, 0.001),
            imu_gyro_noise_stddev: 0.003,
            last_baro_altitude: 0.0,
            last_mag_heading: 0.0,
            last_accel: Vec3::ZERO,
            last_gyro: Vec3::ZERO,
        }
    }
}

pub fn update_gps_noise(
    time: Res<Time>,
    mut query: Query<(&Kinematics, &mut GpsPosition, &mut GpsNoise), With<DroneIdentity>>,
) {
    let dt = time.delta_seconds();
    for (kinematics, mut gps, mut noise) in query.iter_mut() {
        if !noise.enabled {
            continue;
        }

        noise.drift_timer += dt;
        if noise.drift_timer >= noise.drift_change_interval {
            noise.drift_timer = 0.0;
            noise.drift_direction = Vec3::new(
                (fastrand::f32() - 0.5) * 2.0,
                0.0,
                (fastrand::f32() - 0.5) * 2.0,
            ).normalize();
        }

        let drift_dir = noise.drift_direction;
        let drift_rate = noise.drift_rate_mps;
        noise.drift_current += drift_dir * drift_rate * dt;
        let drift_limit = noise.noise_stddev_m * 3.0;
        noise.drift_current = noise.drift_current.clamp(
            -Vec3::splat(drift_limit),
            Vec3::splat(drift_limit),
        );

        if noise.multipath_enabled && kinematics.position.y < noise.multipath_threshold_alt {
            noise.hdop = 2.0 + fastrand::f32() * 3.0;
            noise.vdop = 3.0 + fastrand::f32() * 4.0;
            noise.num_satellites = (6 + fastrand::u8(..4)).min(9);
        } else {
            noise.hdop = 0.8 + fastrand::f32() * 0.4;
            noise.vdop = 1.2 + fastrand::f32() * 0.6;
            noise.num_satellites = 10 + fastrand::u8(..6);
        }

        if noise.fix_lost {
            noise.fix_lost_timer += dt;
            if noise.fix_lost_timer >= noise.fix_lost_duration {
                noise.fix_lost = false;
                noise.fix_lost_timer = 0.0;
            }
            continue;
        }

        let noisy_pos = kinematics.position + noise.drift_current + Vec3::new(
            (fastrand::f32() - 0.5) * noise.noise_stddev_m * 2.0,
            (fastrand::f32() - 0.5) * noise.noise_stddev_m,
            (fastrand::f32() - 0.5) * noise.noise_stddev_m * 2.0,
        );
        noise.last_noisy_position = noisy_pos;
    }
}

pub fn update_sensor_noise(
    time: Res<Time>,
    mut query: Query<(&Kinematics, &mut SensorNoise), With<DroneIdentity>>,
) {
    let dt = time.delta_seconds();
    for (kinematics, mut sensor) in query.iter_mut() {
        let gravity = Vec3::new(0.0, -9.81, 0.0);
        let true_accel = kinematics.velocity * 0.0 + gravity;
        sensor.last_accel = true_accel
            + sensor.imu_accel_bias
            + Vec3::new(
                (fastrand::f32() - 0.5) * sensor.imu_accel_noise_stddev * 2.0,
                (fastrand::f32() - 0.5) * sensor.imu_accel_noise_stddev * 2.0,
                (fastrand::f32() - 0.5) * sensor.imu_accel_noise_stddev * 2.0,
            );

        sensor.last_gyro = kinematics.angular_velocity
            + sensor.imu_gyro_bias
            + Vec3::new(
                (fastrand::f32() - 0.5) * sensor.imu_gyro_noise_stddev * 2.0,
                (fastrand::f32() - 0.5) * sensor.imu_gyro_noise_stddev * 2.0,
                (fastrand::f32() - 0.5) * sensor.imu_gyro_noise_stddev * 2.0,
            );

        sensor.last_baro_altitude = kinematics.position.y
            + sensor.barometer_alt_bias
            + (fastrand::f32() - 0.5) * sensor.barometer_noise_stddev * 2.0;

        let (yaw, _, _) = kinematics.orientation.to_euler(EulerRot::YXZ);
        sensor.last_mag_heading = yaw.to_degrees()
            + (fastrand::f32() - 0.5) * sensor.magnetometer_noise_stddev * 2.0 * 57.2958;

        sensor.imu_accel_bias += Vec3::new(
            (fastrand::f32() - 0.5) * 0.001,
            (fastrand::f32() - 0.5) * 0.001,
            (fastrand::f32() - 0.5) * 0.001,
        ) * dt;
    }
}

pub struct SensorPlugin;

impl Plugin for SensorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (
            update_gps_noise,
            update_sensor_noise,
        ).after(crate::drone::DroneSystemSet::Physics));
    }
}
