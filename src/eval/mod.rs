//! Evaluation framework: metrics collection, trace recording, and event buffering.
//!
//! Sub-modules are exposed directly rather than through a facade because
//! `trace::TraceCollector` and `metrics::MetricsCollector` are used independently
//! by many systems across the crate.

pub mod trace;
pub mod metrics;

use bevy::prelude::*;
use crate::eval::trace::TraceCollector;
use crate::eval::metrics::MetricsCollector;

/// Plugin that registers trace and metrics resources and their update systems.
pub struct EvalPlugin;

impl Plugin for EvalPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TraceCollector>()
            .init_resource::<MetricsCollector>()
            .add_systems(Update, trace::auto_save_traces)
            .add_systems(Update, metrics::update_metrics)
            .add_systems(Update, trace::cleanup_trace_data)
            .add_systems(Update, metrics::cleanup_metrics_data)
            .add_systems(Update, clear_eval_data.after(crate::handle_reload_world));
    }
}

pub fn clear_eval_data(
    mut events: EventReader<crate::events::ClearDroneDataEvent>,
    trace: ResMut<trace::TraceCollector>,
    mut metrics: ResMut<metrics::MetricsCollector>,
) {
    for _ in events.read() {
        if let Ok(mut buf) = trace.event_buffer.lock() {
            buf.clear();
        }
        metrics.drone_metrics.clear();
        metrics.global_start_time = None;
    }
}
