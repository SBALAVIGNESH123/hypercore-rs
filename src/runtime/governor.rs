use crate::metrics::SystemMetrics;
use crate::metrics::events::{dispatch, MetricEvent};
use crate::runtime::{DegradedMode, RuntimeMode, RuntimeState};
use std::collections::VecDeque;
use std::time::Duration;
use tokio::sync::watch;

const METRICS_WINDOW: usize = 10;
const RECOVERY_MARGIN: f32 = 5.0; // Hysteresis margin

#[derive(Clone, Debug, PartialEq)]
pub enum MetricSource {
    LlamaEngine,
    System,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LatencyClass {
    Compute,
    Wait,
}

#[derive(Clone, Debug)]
pub struct EngineMetrics {
    pub tokens_per_sec: f32,
    pub queue_depth: usize,
    pub stalled: bool,
    pub source: MetricSource,
    pub timestamp: std::time::Instant,
    pub latency_class: LatencyClass,
}

impl Default for EngineMetrics {
    fn default() -> Self {
        Self {
            tokens_per_sec: 0.0,
            queue_depth: 0,
            stalled: false,
            source: MetricSource::System,
            timestamp: std::time::Instant::now(),
            latency_class: LatencyClass::Wait,
        }
    }
}

pub struct SafetyGovernor {
    metrics_rx: watch::Receiver<SystemMetrics>,
    engine_rx: watch::Receiver<EngineMetrics>,
    state_tx: watch::Sender<RuntimeState>,
    memory_history: VecDeque<f32>,
}

pub fn evaluate_degraded_mode(sys: &SystemMetrics) -> DegradedMode {
    if sys.memory_pressure_pct > 95.0 {
        DegradedMode::CriticalMemoryPressure
    } else if sys.memory_pressure_pct > 80.0 {
        DegradedMode::MemoryPressure
    } else {
        DegradedMode::Healthy
    }
}

impl SafetyGovernor {
    pub fn new(
        metrics_rx: watch::Receiver<SystemMetrics>,
        engine_rx: watch::Receiver<EngineMetrics>,
        state_tx: watch::Sender<RuntimeState>,
    ) -> Self {
        Self {
            metrics_rx,
            engine_rx,
            state_tx,
            memory_history: VecDeque::with_capacity(METRICS_WINDOW),
        }
    }

    pub async fn run(mut self) {
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        let mut current_mode = RuntimeMode::Running;

        loop {
            interval.tick().await;

            let sys = self.metrics_rx.borrow().clone();
            let _engine = self.engine_rx.borrow().clone();

            if self.memory_history.len() >= METRICS_WINDOW {
                self.memory_history.pop_front();
            }
            self.memory_history
                .push_back(sys.memory_pressure_pct as f32);

            let memory_slope = self.calculate_memory_slope();

            // Deterministic Decision Logic with Hysteresis
            let new_mode = match current_mode {
                RuntimeMode::Running => {
                    if sys.memory_pressure_pct > 95.0 {
                        RuntimeMode::Paused
                    } else if sys.memory_pressure_pct > 85.0 || memory_slope > 2.0 {
                        RuntimeMode::Throttled
                    } else {
                        RuntimeMode::Running
                    }
                }
                RuntimeMode::Throttled => {
                    if sys.memory_pressure_pct > 95.0 {
                        RuntimeMode::Paused
                    } else if sys.memory_pressure_pct < (85.0 - RECOVERY_MARGIN as f64)
                        && memory_slope <= 0.0
                    {
                        RuntimeMode::Running
                    } else {
                        RuntimeMode::Throttled
                    }
                }
                RuntimeMode::Paused => {
                    if sys.memory_pressure_pct < (90.0 - RECOVERY_MARGIN as f64)
                    {
                        RuntimeMode::Throttled
                    } else {
                        RuntimeMode::Paused
                    }
                }
            };

            let degraded_mode = evaluate_degraded_mode(&sys);

            let current_degraded = self.state_tx.borrow().degraded_mode;
            if new_mode != current_mode || degraded_mode != current_degraded {
                if degraded_mode != current_degraded {
                    dispatch(MetricEvent::DegradedModeChanged {
                        old: current_degraded,
                        new: degraded_mode,
                    });
                }
                current_mode = new_mode;
                let _ = self.state_tx.send(RuntimeState {
                    mode: current_mode,
                    active_tokens: 0,
                    max_tokens: 512,
                    degraded_mode,
                });
            }
        }
    }

    fn calculate_memory_slope(&self) -> f32 {
        if self.memory_history.len() < 2 {
            return 0.0;
        }
        let first = self.memory_history.front().copied().unwrap_or(0.0);
        let last = self.memory_history.back().copied().unwrap_or(0.0);
        last - first
    }
}

