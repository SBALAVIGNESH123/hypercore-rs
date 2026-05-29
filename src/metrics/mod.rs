pub mod events;
pub mod prometheus_sink;
pub mod stats;
pub mod telemetry;

use sysinfo::System;
use tokio::sync::watch;
use tokio::time::{sleep, Duration};

#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct SystemMetrics {
    pub total_memory: u64,
    pub used_memory: u64,
    pub free_memory: u64,
    pub total_swap: u64,
    pub used_swap: u64,
    pub memory_pressure_pct: f64,
    pub swap_usage_pct: f64,
    pub timestamp_ms: u128,
}

pub struct Watchdog {
    sys: System,
    tx: watch::Sender<SystemMetrics>,
    interval: Duration,
}

impl Watchdog {
    pub fn new(tx: watch::Sender<SystemMetrics>, interval: Duration) -> Self {
        Self {
            sys: System::new_all(),
            tx,
            interval,
        }
    }

    /// Pure publisher: Polls metrics and updates the watch channel (prevents backpressure)
    pub async fn start(mut self) {
        // Initial refresh
        self.sys.refresh_memory();

        loop {
            self.sys.refresh_memory();

            let total_mem = self.sys.total_memory();
            let used_mem = self.sys.used_memory();
            let free_mem = self.sys.free_memory();

            let total_swap = self.sys.total_swap();
            let used_swap = self.sys.used_swap();

            let pressure = if total_mem > 0 {
                (used_mem as f64 / total_mem as f64) * 100.0
            } else {
                0.0
            };

            let swap_pct = if total_swap > 0 {
                (used_swap as f64 / total_swap as f64) * 100.0
            } else {
                0.0
            };

            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis();

            let metrics = SystemMetrics {
                total_memory: total_mem,
                used_memory: used_mem,
                free_memory: free_mem,
                total_swap,
                used_swap,
                memory_pressure_pct: pressure,
                swap_usage_pct: swap_pct,
                timestamp_ms: timestamp,
            };

            crate::metrics::events::dispatch(crate::metrics::events::MetricEvent::MemoryUpdated {
                rss_bytes: used_mem,
            });

            if self.tx.send(metrics).is_err() {
                break;
            }

            sleep(self.interval).await;
        }
    }
}
