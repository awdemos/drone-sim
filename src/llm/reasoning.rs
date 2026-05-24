use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::core::types::{DroneId, SimTimestamp};

/// A single step in an LLM reasoning chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningStep {
    pub step_id: String,
    pub step_type: String, // "observation", "thought", "action", "reflection"
    pub content: String,
    pub timestamp: SimTimestamp,
    pub confidence: f32,
    pub metadata: HashMap<String, serde_json::Value>,
}

/// A complete reasoning trace for a drone
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningChain {
    pub drone_id: DroneId,
    pub steps: Vec<ReasoningStep>,
    pub start_time: SimTimestamp,
    pub end_time: Option<SimTimestamp>,
}

/// Resource holding all reasoning traces
#[derive(Resource, Default, Serialize, Deserialize)]
pub struct ReasoningTrace {
    pub chains: HashMap<DroneId, Vec<ReasoningChain>>,
    pub active_chains: HashMap<DroneId, ReasoningChain>,
}

impl ReasoningTrace {
    pub fn start_chain(&mut self, drone_id: DroneId) {
        if let Some(mut existing) = self.active_chains.remove(&drone_id) {
            existing.end_time = Some(SimTimestamp::now());
            self.chains.entry(drone_id).or_default().push(existing);
        }
        let chain = ReasoningChain {
            drone_id,
            steps: Vec::new(),
            start_time: SimTimestamp::now(),
            end_time: None,
        };
        self.active_chains.insert(drone_id, chain);
    }

    pub fn add_step(
        &mut self,
        drone_id: DroneId,
        step_id: String,
        step_type: String,
        content: String,
        confidence: f32,
    ) {
        let step = ReasoningStep {
            step_id,
            step_type,
            content,
            timestamp: SimTimestamp::now(),
            confidence,
            metadata: HashMap::new(),
        };

        if let Some(chain) = self.active_chains.get_mut(&drone_id) {
            chain.steps.push(step);
        } else {
            self.start_chain(drone_id);
            if let Some(chain) = self.active_chains.get_mut(&drone_id) {
                chain.steps.push(step);
            }
        }
    }

    #[allow(dead_code)]
    pub fn end_chain(&mut self, drone_id: DroneId) {
        if let Some(mut chain) = self.active_chains.remove(&drone_id) {
            chain.end_time = Some(SimTimestamp::now());
            self.chains.entry(drone_id).or_default().push(chain);
        }
    }

    #[allow(dead_code)]
    pub fn get_latest_chain(&self, drone_id: DroneId) -> Option<&ReasoningChain> {
        self.active_chains.get(&drone_id)
            .or_else(|| self.chains.get(&drone_id)?.last())
    }

    pub fn export_to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
}

pub fn cleanup_reasoning_data(
    mut events: EventReader<crate::drone::DroneDestroyedEvent>,
    mut reasoning: ResMut<ReasoningTrace>,
) {
    for event in events.read() {
        reasoning.chains.remove(&event.drone_id);
        reasoning.active_chains.remove(&event.drone_id);
        debug!("Cleaned up ReasoningTrace for drone {:?}", event.drone_id);
    }
}

/// An LLM decision for a drone
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmDecision {
    pub drone_id: DroneId,
    pub timestamp: SimTimestamp,
    pub action: String,
    pub reasoning: String,
    pub duration_secs: f32,
    pub confidence: f32,
    pub raw_response: Option<String>,
}
