use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::core::types::DroneId;
use crate::drone::{DroneIdentity, Kinematics};

/// Metrics collected per drone
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DroneMetrics {
    pub total_distance_m: f64,
    pub max_altitude_m: f64,
    pub min_altitude_m: f64,
    pub flight_time_secs: f64,
    pub average_speed_ms: f64,
    pub max_speed_ms: f64,
    pub waypoint_success_rate: f32,
    pub llm_decisions_count: u32,
    pub collisions: u32,
    pub battery_efficiency_mah_per_km: f32,
    pub path_deviation_m: f64,
}

/// Resource holding all metrics
#[derive(Resource, Default, Serialize, Deserialize)]
pub struct MetricsCollector {
    pub drone_metrics: HashMap<DroneId, DroneMetrics>,
    pub global_start_time: Option<f64>,
}

impl MetricsCollector {
    pub fn get_or_create(&mut self, drone_id: DroneId) -> &mut DroneMetrics {
        self.drone_metrics.entry(drone_id).or_default()
    }

    #[allow(dead_code)]
    pub fn export_to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
}

/// Update metrics each frame
pub fn update_metrics(
    time: Res<Time>,
    mut metrics: ResMut<MetricsCollector>,
    drone_query: Query<(&DroneIdentity, &Kinematics)>,
) {
    let dt = time.delta_seconds() as f64;

    if metrics.global_start_time.is_none() {
        metrics.global_start_time = Some(time.elapsed_seconds_f64());
    }

    for (identity, kinematics) in drone_query.iter() {
        let m = metrics.get_or_create(identity.id);
        
        let speed = kinematics.velocity.length() as f64;
        let alt = kinematics.position.y as f64;
        
        m.flight_time_secs += dt;
        m.total_distance_m += speed * dt;
        m.max_altitude_m = m.max_altitude_m.max(alt);
        if m.min_altitude_m == 0.0 && m.flight_time_secs <= dt {
            m.min_altitude_m = alt;
        } else {
            m.min_altitude_m = m.min_altitude_m.min(alt);
        }
        m.max_speed_ms = m.max_speed_ms.max(speed);
        
        if m.flight_time_secs > 0.0 {
            m.average_speed_ms = m.total_distance_m / m.flight_time_secs;
        }
    }
}

pub fn cleanup_metrics_data(
    mut events: EventReader<crate::drone::DroneDestroyedEvent>,
    mut metrics: ResMut<MetricsCollector>,
) {
    for event in events.read() {
        metrics.drone_metrics.remove(&event.drone_id);
        debug!("Cleaned up MetricsCollector for drone {:?}", event.drone_id);
    }
}
