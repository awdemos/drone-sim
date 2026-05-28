use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

/// Unique identifier for a drone
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Component, Serialize, Deserialize)]
pub struct DroneId(pub Uuid);

impl DroneId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for DroneId {
    fn default() -> Self {
        Self::new()
    }
}

/// Simulation timestamp
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SimTimestamp {
    pub nanos: u64,
}

impl SimTimestamp {
    pub fn now() -> Self {
        use std::time::SystemTime;
        let since_epoch = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default();
        Self {
            nanos: since_epoch.as_nanos() as u64,
        }
    }

    #[allow(dead_code)]
    pub fn from_duration(d: Duration) -> Self {
        Self {
            nanos: d.as_nanos() as u64,
        }
    }
}

/// A 3D pose in the simulation world
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Pose {
    pub position: Vec3,
    pub orientation: Quat,
    pub velocity: Vec3,
    pub angular_velocity: Vec3,
}

/// GPS coordinate (WGS84)
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct GpsCoord {
    pub latitude: f64,
    pub longitude: f64,
    pub altitude_msl: f64,
}

/// ENU (East-North-Up) local coordinate frame
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct EnuCoord {
    pub east: f64,
    pub north: f64,
    pub up: f64,
}

/// Flight mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FlightMode {
    Manual,
    Stabilize,
    AltHold,
    Loiter,
    Guided,
    Auto,
    Rtl,
    Land,
}

impl Default for FlightMode {
    fn default() -> Self {
        FlightMode::Stabilize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DroneType {
    TinyWhoop,
    Racing5Inch,
    Racing7Inch,
    CineWhoop,
    MavicStyle,
    InspireStyle,
    Hexacopter,
    Octocopter,
    VtolFixedWing,
    ToyDrone,
}

impl Default for DroneType {
    fn default() -> Self {
        DroneType::MavicStyle
    }
}

/// Motor type for drone propulsion
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MotorType {
    Brushed,
    Brushless,
}

/// Waypoint for autonomous navigation
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Waypoint {
    pub latitude: f64,
    pub longitude: f64,
    pub altitude_agl: f64,
    pub hold_time_secs: f32,
}

/// Mission definition
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Mission {
    pub name: String,
    pub waypoints: Vec<Waypoint>,
    pub loop_mission: bool,
}

/// Sensor reading snapshot
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorSnapshot {
    pub timestamp: SimTimestamp,
    pub gps: GpsCoord,
    pub pose: Pose,
    pub battery_percent: f32,
    pub flight_mode: FlightMode,
}

/// An event in the simulation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SimEvent {
    DroneSpawned {
        drone_id: DroneId,
        timestamp: SimTimestamp,
        initial_gps: GpsCoord,
    },
    ModeChanged {
        drone_id: DroneId,
        timestamp: SimTimestamp,
        old_mode: FlightMode,
        new_mode: FlightMode,
    },
    WaypointReached {
        drone_id: DroneId,
        timestamp: SimTimestamp,
        waypoint_index: usize,
    },
    LlmDecision {
        drone_id: DroneId,
        timestamp: SimTimestamp,
        decision: String,
        confidence: f32,
    },
    ObstacleDetected {
        drone_id: DroneId,
        timestamp: SimTimestamp,
        obstacle_position: Vec3,
        distance: f32,
    },
    Collision {
        drone_id: DroneId,
        timestamp: SimTimestamp,
        position: Vec3,
    },
}

/// FAA Airspace restriction zone type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AirspaceRestrictionType {
    NoFly,
    HeightRestricted,
    Warning,
}

/// Geometry for an airspace restriction zone
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AirspaceGeometry {
    Circle { center_lat: f64, center_lon: f64, radius_meters: f64 },
    Polygon { vertices: Vec<(f64, f64)> },
}

/// An airspace restriction zone (airport no-fly, height limit, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirspaceZone {
    pub name: String,
    pub zone_type: AirspaceRestrictionType,
    pub geometry: AirspaceGeometry,
    pub min_altitude_ft: Option<f64>,
    pub max_altitude_ft: Option<f64>,
    pub source: String,
}

/// A user-drawn geofence polygon on the map
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserGeofence {
    pub name: String,
    pub vertices: Vec<(f64, f64)>,
    pub max_altitude_ft: Option<f64>,
}
