pub mod client;
pub mod reasoning;

use bevy::prelude::*;
use bevy::tasks::AsyncComputeTaskPool;
use std::sync::Arc;
use crate::core::config::LlmConfig;
use crate::core::types::{SimEvent, SimTimestamp};
use crate::drone::{DroneIdentity, Kinematics, FlightControl, Battery, Health, MissionState};
use crate::drone::types::DroneTypeRegistry;
use crate::eval::trace::TraceCollector;
use crate::llm::client::{LlmClient, LlmRequest, LlmResponse};
use crate::llm::reasoning::{LlmDecision, ReasoningTrace};

/// Plugin for LLM integration
pub struct LlmPlugin;

impl Plugin for LlmPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LlmDecisionQueue>()
            .init_resource::<LlmClientResource>()
            .init_resource::<LlmTimer>()
            .add_systems(Startup, setup_llm_client)
            .add_systems(Update, spawn_llm_tasks)
            .add_systems(Update, poll_llm_tasks)
            .add_systems(Update, apply_llm_decisions)
            .add_systems(Update, reasoning::cleanup_reasoning_data)
            .add_systems(Update, clear_llm_data.after(crate::handle_reload_world));
    }
}

pub fn clear_llm_data(
    mut events: EventReader<crate::events::ClearDroneDataEvent>,
    mut reasoning: ResMut<ReasoningTrace>,
) {
    for _ in events.read() {
        reasoning.active_chains.clear();
    }
}

/// Resource holding the LLM client
#[derive(Resource, Default)]
pub struct LlmClientResource {
    pub client: Option<std::sync::Arc<dyn LlmClient>>,
}

/// Resource holding pending LLM decisions
#[derive(Resource, Default)]
pub struct LlmDecisionQueue {
    pub pending: Vec<LlmDecision>,
}

/// Timer to rate-limit LLM queries
#[derive(Resource)]
pub struct LlmTimer {
    pub last_query: f32,
}

impl Default for LlmTimer {
    fn default() -> Self {
        Self { last_query: -999.0 }
    }
}

/// Component representing an in-flight LLM request
#[derive(Component)]
pub struct LlmTask {
    pub drone_id: crate::core::types::DroneId,
    #[allow(dead_code)]
    pub start_time: SimTimestamp,
}

/// Wrapper so blocking result can be a Component
#[derive(Component)]
pub struct LlmTaskResult {
    pub result: Result<LlmResponse, anyhow::Error>,
}

/// Wrapper so Task can be a Component
#[derive(Component)]
pub struct LlmTaskHandle {
    pub task: bevy::tasks::Task<LlmTaskResult>,
}

fn setup_llm_client(mut res: ResMut<LlmClientResource>, config: Res<LlmConfig>) {
    if !config.enabled {
        return;
    }
    let api_url = config.resolve_api_url();
    let api_key = config.resolve_api_key();
    res.client = Some(crate::llm::client::create_client(
        &config.provider,
        config.model.clone(),
        api_url,
        api_key,
    ));
}

fn spawn_llm_tasks(
    time: Res<Time>,
    config: Res<LlmConfig>,
    ui_prefs: Res<crate::ui::UiPreferences>,
    registry: Res<DroneTypeRegistry>,
    query: Query<(&DroneIdentity, &Kinematics, &FlightControl, &Battery, &Health, &MissionState)>,
    client_res: Res<LlmClientResource>,
    mut timer: ResMut<LlmTimer>,
    mut commands: Commands,
) {
    if !config.enabled || !ui_prefs.llm_enabled || client_res.client.is_none() {
        return;
    }

    let now = time.elapsed_seconds();
    if now - timer.last_query < config.decision_interval_secs {
        return;
    }
    timer.last_query = now;

    let Some(client) = client_res.client.as_ref() else {
        return;
    };
    let system_prompt = config.system_prompt.clone();

    for (identity, kinematics, flight_control, battery, _health, mission_state) in query.iter() {
        let spec = registry.specs.get(&identity.drone_type).unwrap();
        let mission_context = mission_state.mission.as_ref().map(|m| serde_json::json!({
            "waypoint_count": m.waypoints.len(),
            "current_waypoint": mission_state.current_waypoint,
            "looping": m.loop_mission,
        }));

        let context = serde_json::json!({
            "drone_id": identity.id,
            "drone_type": format!("{:?}", identity.drone_type),
            "capabilities": {
                "max_speed_ms": spec.flight_controller.max_speed_ms,
                "max_climb_rate_ms": spec.flight_controller.max_climb_rate_ms,
                "max_flight_time_min": spec.battery.max_flight_time_secs / 60.0,
                "camera_resolution": format!("{}x{}", spec.camera.resolution.0, spec.camera.resolution.1),
                "has_gimbal": spec.camera.has_gimbal,
                "arm_count": spec.airframe.arm_count,
                "motor_type": format!("{:?}", spec.engine.motor_type),
            },
            "telemetry": {
                "position": [kinematics.position.x, kinematics.position.y, kinematics.position.z],
                "velocity": [kinematics.velocity.x, kinematics.velocity.y, kinematics.velocity.z],
                "altitude_msl": 0.0,
                "battery": battery.percent,
                "flight_mode": format!("{:?}", flight_control.mode),
            },
            "mission": mission_context,
            "camera_available": true,
        });

        let user_prompt = format!(
            "Telemetry: {}. What action should the drone take? Respond in JSON.",
            context.to_string()
        );

        let client_clone = Arc::clone(client);

        let request = LlmRequest {
            system_prompt: system_prompt.clone(),
            user_prompt,
            max_tokens: config.max_tokens,
            temperature: config.temperature,
            images: Vec::new(),
        };

        let task = AsyncComputeTaskPool::get().spawn(async move {
            let result = client_clone.prompt(request).await.map_err(|e| anyhow::anyhow!(e));
            LlmTaskResult { result }
        });

        commands.spawn((
            LlmTask {
                drone_id: identity.id,
                start_time: SimTimestamp::now(),
            },
            LlmTaskHandle { task },
        ));
    }
}

fn poll_llm_tasks(
    mut commands: Commands,
    mut tasks: Query<(Entity, &LlmTask, &mut LlmTaskHandle)>,
    mut trace: ResMut<TraceCollector>,
    mut reasoning_traces: ResMut<ReasoningTrace>,
    mut queue: ResMut<LlmDecisionQueue>,
) {
    for (entity, task_meta, mut handle) in tasks.iter_mut() {
        if let Some(result) = bevy::tasks::block_on(bevy::tasks::futures_lite::future::poll_once(&mut handle.task)) {
            commands.entity(entity).despawn();

            let decision = match result.result {
                Ok(response) => {
                    let (action, reasoning, confidence) = parse_llm_response(&response.text);
                    LlmDecision {
                        drone_id: task_meta.drone_id,
                        timestamp: SimTimestamp::now(),
                        action,
                        reasoning,
                        duration_secs: 2.0,
                        confidence,
                        raw_response: Some(response.text),
                    }
                }
                Err(e) => {
                    eprintln!("LLM request failed: {}", e);
                    LlmDecision {
                        drone_id: task_meta.drone_id,
                        timestamp: SimTimestamp::now(),
                        action: "hover".into(),
                        reasoning: format!("LLM error: {}", e),
                        duration_secs: 2.0,
                        confidence: 0.0,
                        raw_response: None,
                    }
                }
            };

            reasoning_traces.add_step(
                task_meta.drone_id,
                format!("{:?}", task_meta.drone_id.0),
                "observation".into(),
                "Received telemetry update".into(),
                1.0,
            );
            reasoning_traces.add_step(
                task_meta.drone_id,
                format!("{:?}", task_meta.drone_id.0),
                "thought".into(),
                decision.reasoning.clone(),
                decision.confidence,
            );
            reasoning_traces.add_step(
                task_meta.drone_id,
                format!("{:?}", task_meta.drone_id.0),
                "action".into(),
                decision.action.clone(),
                decision.confidence,
            );

            trace.record_event(SimEvent::LlmDecision {
                drone_id: task_meta.drone_id,
                timestamp: SimTimestamp::now(),
                decision: decision.action.clone(),
                confidence: decision.confidence,
            });

            queue.pending.push(decision);
        }
    }
}

fn parse_llm_response(text: &str) -> (String, String, f32) {
    let json_start = text.find('{');
    let json_end = text.rfind('}');
    
    if let (Some(start), Some(end)) = (json_start, json_end) {
        let json_str = &text[start..=end];
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(json_str) {
            let action = json["action"].as_str().unwrap_or("hover").to_string();
            let reasoning = json["reasoning"].as_str().unwrap_or("No reasoning provided").to_string();
            let confidence = json["confidence"].as_f64().unwrap_or(1.0) as f32;
            return (action, reasoning, confidence);
        }
    }
    
    ("hover".into(), text.to_string(), 0.5)
}

/// Apply LLM decisions to drones
fn apply_llm_decisions(
    mut queue: ResMut<LlmDecisionQueue>,
    mut query: Query<(&DroneIdentity, &mut Kinematics, &mut FlightControl, &mut MissionState)>,
) {
    let decisions: Vec<LlmDecision> = queue.pending.drain(..).collect();
    
    for decision in decisions {
        for (identity, _kinematics, mut flight_control, mut mission_state) in query.iter_mut() {
            if identity.id == decision.drone_id {
                match decision.action.as_str() {
                    "move_forward" => {
                        let forward = _kinematics.orientation * Vec3::Z;
                        flight_control.target_velocity = forward * 5.0;
                        flight_control.mode = crate::core::types::FlightMode::Guided;
                    }
                    "move_back" => {
                        let back = _kinematics.orientation * -Vec3::Z;
                        flight_control.target_velocity = back * 5.0;
                        flight_control.mode = crate::core::types::FlightMode::Guided;
                    }
                    "move_left" => {
                        let left = _kinematics.orientation * -Vec3::X;
                        flight_control.target_velocity = left * 5.0;
                        flight_control.mode = crate::core::types::FlightMode::Guided;
                    }
                    "move_right" => {
                        let right = _kinematics.orientation * Vec3::X;
                        flight_control.target_velocity = right * 5.0;
                        flight_control.mode = crate::core::types::FlightMode::Guided;
                    }
                    "ascend" => {
                        flight_control.target_velocity.y = 2.0;
                        flight_control.mode = crate::core::types::FlightMode::Guided;
                    }
                    "descend" => {
                        flight_control.target_velocity.y = -2.0;
                        flight_control.mode = crate::core::types::FlightMode::Guided;
                    }
                    "rotate_cw" => {
                        flight_control.angular_thrust.y = -1.0;
                        flight_control.mode = crate::core::types::FlightMode::Guided;
                    }
                    "rotate_ccw" => {
                        flight_control.angular_thrust.y = 1.0;
                        flight_control.mode = crate::core::types::FlightMode::Guided;
                    }
                    "hover" | "stop" => {
                        flight_control.target_velocity = Vec3::ZERO;
                        flight_control.angular_thrust = Vec3::ZERO;
                        flight_control.mode = crate::core::types::FlightMode::Guided;
                    }
                    "land" => {
                        flight_control.mode = crate::core::types::FlightMode::Land;
                    }
                    "navigate_to" => {
                        if let Some(raw) = &decision.raw_response {
                            if let (Some(start), Some(end)) = (raw.find('{'), raw.rfind('}')) {
                                let json_str = &raw[start..=end];
                                if let Ok(json) = serde_json::from_str::<serde_json::Value>(json_str) {
                                    if let (Some(lat), Some(lon), Some(alt)) = (
                                        json["target_lat"].as_f64(),
                                        json["target_lon"].as_f64(),
                                        json["target_alt_agl"].as_f64(),
                                    ) {
                                        mission_state.mission = Some(crate::core::types::Mission {
                                            name: "LLM Navigation".to_string(),
                                            waypoints: vec![crate::core::types::Waypoint {
                                                latitude: lat,
                                                longitude: lon,
                                                altitude_agl: alt,
                                                hold_time_secs: 0.0,
                                            }],
                                            loop_mission: false,
                                        });
                                        mission_state.current_waypoint = 0;
                                        flight_control.mode = crate::core::types::FlightMode::Auto;
                                    }
                                }
                            }
                        }
                    }
                    "follow_mission" => {
                        if mission_state.mission.is_some() {
                            flight_control.mode = crate::core::types::FlightMode::Auto;
                        }
                    }
                    _ => {
                        flight_control.target_velocity = Vec3::ZERO;
                    }
                }
            }
        }
    }
}
