use crate::core::types::DroneType;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Resource)]
pub struct SimConfig {
    pub window: WindowConfig,
    pub world: WorldConfig,
    pub drone: DroneConfig,
    pub physics: PhysicsConfig,
    pub llm: LlmConfig,
    pub eval: EvalConfig,
}

impl Default for SimConfig {
    fn default() -> Self {
        Self {
            window: WindowConfig::default(),
            world: WorldConfig::default(),
            drone: DroneConfig::default(),
            physics: PhysicsConfig::default(),
            llm: LlmConfig::default(),
            eval: EvalConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Resource)]
pub struct WindowConfig {
    pub title: String,
    pub width: u32,
    pub height: u32,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            title: "Drone Flight Simulator".into(),
            width: 1600,
            height: 900,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Resource)]
pub struct WorldConfig {
    /// Origin latitude for ENU frame (degrees)
    pub origin_lat: f64,
    /// Origin longitude for ENU frame (degrees)
    pub origin_lon: f64,
    /// Terrain size in meters
    pub terrain_size_m: f32,
    /// Terrain resolution (vertices per side)
    pub terrain_resolution: u32,
    /// Path to heightmap image (optional)
    pub heightmap_path: Option<PathBuf>,
    /// Path to OSM PBF file (optional)
    pub osm_path: Option<PathBuf>,
    /// Maximum building height in meters (for procedural generation)
    pub max_building_height: f32,
    /// Enable procedural trees
    pub procedural_vegetation: bool,
}

impl Default for WorldConfig {
    fn default() -> Self {
        Self {
            origin_lat: 37.7749,
            origin_lon: -122.4194,
            terrain_size_m: 2000.0,
            terrain_resolution: 256,
            heightmap_path: None,
            osm_path: Some(PathBuf::from("data/osm/map.json")),
            max_building_height: 30.0,
            procedural_vegetation: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Resource)]
pub struct DroneConfig {
    pub spawn_count: usize,
    pub spawn_altitude_agl: f32,
    pub default_drone_type: DroneType,
    pub per_drone_types: Vec<DroneType>,
    pub user_body_color: Option<[f32; 3]>,
    pub user_arm_color: Option<[f32; 3]>,
}

impl Default for DroneConfig {
    fn default() -> Self {
        Self {
            spawn_count: 1,
            spawn_altitude_agl: 10.0,
            default_drone_type: DroneType::MavicStyle,
            per_drone_types: Vec::new(),
            user_body_color: None,
            user_arm_color: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Resource)]
pub struct PhysicsConfig {
    /// Wind speed in m/s (surface)
    pub wind_speed_ms: f32,
    /// Wind direction in degrees (0=North, 90=East)
    pub wind_direction_deg: f32,
    /// Turbulence intensity (0..1, fraction of wind speed)
    pub turbulence: f32,
    /// Wind gust factor (0..1, intensity of sudden gusts)
    pub gust_factor: f32,
    /// Sea-level air density (kg/m³) — normalized reference, 1.0 = standard
    pub sea_level_density: f32,
    /// Atmospheric scale height in meters (density e-fold)
    pub density_scale_height_m: f32,
    /// Altitude offset for density calc (e.g., if sea-level is not y=0)
    pub sea_level_offset_m: f32,
    /// Rain intensity (0..1) — increases drag and reduces visibility
    pub rain_intensity: f32,
    /// Icing factor (0..1) — reduces lift and increases mass
    pub icing_factor: f32,
}

impl Default for PhysicsConfig {
    fn default() -> Self {
        Self {
            wind_speed_ms: 0.0,
            wind_direction_deg: 0.0,
            turbulence: 0.1,
            gust_factor: 0.0,
            sea_level_density: 1.0,
            density_scale_height_m: 8400.0,
            sea_level_offset_m: 0.0,
            rain_intensity: 0.0,
            icing_factor: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Resource)]
pub struct LlmConfig {
    pub enabled: bool,
    /// Provider: "ollama", "openai", "moonshot", "kimi"
    pub provider: String,
    pub model: String,
    pub api_url: String,
    pub api_key: Option<String>,
    pub max_tokens: u32,
    pub temperature: f32,
    /// How often (in seconds) the LLM is queried for decisions
    pub decision_interval_secs: f32,
    /// Send camera frames to visual LLM
    pub visual_mode: bool,
    /// Prompt template for drone decisions
    pub system_prompt: String,
}

impl LlmConfig {
    /// Resolve API key from explicit config or environment variable
    pub fn resolve_api_key(&self) -> Option<String> {
        if let Some(ref key) = self.api_key {
            if !key.is_empty() && !key.starts_with("${") {
                return Some(key.clone());
            }
        }
        // Try provider-specific env vars
        let env_var = match self.provider.as_str() {
            "moonshot" => "MOONSHOT_API_KEY",
            "kimi" => "KIMI_API_KEY",
            "openai" => "OPENAI_API_KEY",
            _ => return None,
        };
        std::env::var(env_var).ok()
    }

    pub fn resolve_api_url(&self) -> String {
        if !self.api_url.is_empty() {
            return self.api_url.clone();
        }
        match self.provider.as_str() {
            "moonshot" | "kimi" => "https://api.moonshot.cn/v1/chat/completions".into(),
            "openai" => "https://api.openai.com/v1/chat/completions".into(),
            "ollama" => "http://localhost:11434/api/generate".into(),
            _ => self.api_url.clone(),
        }
    }
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            provider: "demo".into(),
            model: "llava:13b".into(),
            api_url: "http://localhost:11434/api/generate".into(),
            api_key: None,
            max_tokens: 512,
            temperature: 0.7,
            decision_interval_secs: 2.0,
            visual_mode: true,
            system_prompt: concat!(
                "You are the AI pilot of a quadcopter drone. ",
                "You receive visual input from the drone camera and telemetry data. ",
                "You can navigate waypoints, avoid obstacles, and respect airspace restrictions. ",
                "Respond with a JSON object containing: ",
                "'action' (one of: hover, move_forward, move_back, move_left, move_right, ascend, descend, rotate_cw, rotate_ccw, land, navigate_to, follow_mission), ",
                "'reasoning' (brief explanation), ",
                "'duration_secs' (how long to execute the action), ",
                "For navigate_to: include 'target_lat', 'target_lon', 'target_alt_agl'."
            )
            .into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Resource)]
pub struct EvalConfig {
    pub enabled: bool,
    /// Directory to save traces
    pub output_dir: PathBuf,
    /// Capture camera frames in traces
    pub capture_frames: bool,
    /// Frame capture interval (every N seconds)
    pub capture_interval_secs: f32,
    /// Max trace size in MB before rotation
    pub max_trace_size_mb: u64,
}

impl Default for EvalConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            output_dir: PathBuf::from("./traces"),
            capture_frames: false,
            capture_interval_secs: 1.0,
            max_trace_size_mb: 100,
        }
    }
}

pub fn load_config(path: Option<&std::path::Path>) -> anyhow::Result<SimConfig> {
    if let Some(p) = path {
        let content = std::fs::read_to_string(p)?;
        let config: SimConfig = toml::from_str(&content)?;
        Ok(config)
    } else if std::path::Path::new("config/sim.toml").exists() {
        let content = std::fs::read_to_string("config/sim.toml")?;
        let config: SimConfig = toml::from_str(&content)?;
        Ok(config)
    } else {
        Ok(SimConfig::default())
    }
}

pub fn save_default_config(path: &std::path::Path) -> anyhow::Result<()> {
    let config = SimConfig::default();
    let content = toml::to_string_pretty(&config)?;
    std::fs::write(path, content)?;
    Ok(())
}

pub fn save_config(path: &std::path::Path, config: &SimConfig) -> anyhow::Result<()> {
    let content = toml::to_string_pretty(config)?;
    std::fs::write(path, content)?;
    Ok(())
}
