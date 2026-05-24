use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use crate::core::config::EvalConfig;
use crate::core::types::{SimEvent, SimTimestamp};

/// A complete simulation trace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimTrace {
    pub trace_id: String,
    pub start_time: SimTimestamp,
    pub events: Vec<SimEvent>,
    pub metadata: TraceMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceMetadata {
    pub version: String,
    pub simulator: String,
    pub config_hash: String,
}

impl Default for TraceMetadata {
    fn default() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").into(),
            simulator: "drone-sim".into(),
            config_hash: String::new(),
        }
    }
}

/// Resource for collecting simulation traces
#[derive(Resource)]
pub struct TraceCollector {
    pub current_trace: Arc<Mutex<SimTrace>>,
    pub event_buffer: Arc<Mutex<VecDeque<SimEvent>>>,
    pub output_dir: PathBuf,
    pub enabled: bool,
    pub capture_frames: bool,
    pub max_size_mb: u64,
    pub frames: Arc<Mutex<Vec<FrameCapture>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameCapture {
    pub timestamp: SimTimestamp,
    pub drone_id: crate::core::types::DroneId,
    pub image_data: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl Default for TraceCollector {
    fn default() -> Self {
        Self::new(&EvalConfig::default())
    }
}

impl TraceCollector {
    pub fn new(config: &EvalConfig) -> Self {
        let _ = fs::create_dir_all(&config.output_dir);
        
        Self {
            current_trace: Arc::new(Mutex::new(SimTrace {
                trace_id: format!("trace-{}", chrono::Utc::now().timestamp()),
                start_time: SimTimestamp::now(),
                events: Vec::new(),
                metadata: TraceMetadata::default(),
            })),
            event_buffer: Arc::new(Mutex::new(VecDeque::new())),
            output_dir: config.output_dir.clone(),
            enabled: config.enabled,
            capture_frames: config.capture_frames,
            max_size_mb: config.max_trace_size_mb,
            frames: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn record_event(&mut self, event: SimEvent) {
        if !self.enabled {
            return;
        }

        match self.event_buffer.lock() {
            Ok(mut buffer) => buffer.push_back(event),
            Err(e) => error!("Event buffer mutex poisoned, dropping event: {}", e),
        }
    }

    #[allow(dead_code)]
    pub fn record_frame(&mut self, frame: FrameCapture) {
        if !self.enabled || !self.capture_frames {
            return;
        }

        match self.frames.lock() {
            Ok(mut frames) => frames.push(frame),
            Err(e) => error!("Frames mutex poisoned, dropping frame: {}", e),
        }
    }

    pub fn flush(&self) -> anyhow::Result<()> {
        let trace = {
            let mut trace = self.current_trace.lock().map_err(|e| anyhow::anyhow!("Trace mutex poisoned: {}", e))?;
            let mut buffer = self.event_buffer.lock().map_err(|e| anyhow::anyhow!("Event buffer mutex poisoned: {}", e))?;
            trace.events.extend(buffer.drain(..));
            trace.clone()
        };

        let filename = self.output_dir.join(format!("{}.json", trace.trace_id));
        let json = serde_json::to_string_pretty(&trace)?;
        fs::write(&filename, json)?;

        // Save frames separately if present
        let frames = {
            let mut frames_guard = self.frames.lock().map_err(|e| anyhow::anyhow!("Frames mutex poisoned: {}", e))?;
            let drained = std::mem::take(&mut *frames_guard);
            drained
        };
        if !frames.is_empty() {
            let frames_dir = self.output_dir.join(format!("{}_frames", trace.trace_id));
            let _ = fs::create_dir_all(&frames_dir);
            
            for (i, frame) in frames.iter().enumerate() {
                let frame_file = frames_dir.join(format!("frame_{:06}.json", i));
                let frame_json = serde_json::to_string_pretty(frame)?;
                let _ = fs::write(&frame_file, frame_json);
            }
        }

        {
            let mut trace = self.current_trace.lock().map_err(|e| anyhow::anyhow!("Trace mutex poisoned: {}", e))?;
            trace.events.clear();
        }

        info!("Trace saved to {:?}", filename);
        Ok(())
    }

    pub fn get_trace_size_estimate(&self) -> u64 {
        let events = self.event_buffer.lock().map(|b| b.len()).unwrap_or(0) as u64;
        let frames = self.frames.lock().map(|f| f.len()).unwrap_or(0) as u64;
        events.saturating_mul(256).saturating_add(frames.saturating_mul(640 * 480 * 4))
    }
}

/// System to auto-save traces periodically
pub fn auto_save_traces(
    trace_collector: ResMut<TraceCollector>,
) {
    let size = trace_collector.get_trace_size_estimate();
    let max_bytes = trace_collector.max_size_mb * 1024 * 1024;

    if size > max_bytes {
        if let Err(e) = trace_collector.flush() {
            error!("Failed to auto-save trace: {}", e);
        }
    }
}

pub fn cleanup_trace_data(
    mut events: EventReader<crate::drone::DroneDestroyedEvent>,
    trace: ResMut<TraceCollector>,
) {
    for event in events.read() {
        if let Ok(mut frames) = trace.frames.lock() {
            frames.retain(|f| f.drone_id != event.drone_id);
        }
        debug!("Cleaned up TraceCollector for drone {:?}", event.drone_id);
    }
}
