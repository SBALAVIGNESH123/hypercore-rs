use crate::runtime::DegradedMode;

#[derive(Debug, Clone)]
pub enum MetricEvent {
    // Queue & Throughput events
    RequestEnqueued,
    RequestAdmitted,
    TokenGenerated {
        count: usize,
    },

    // Rejections & Drops
    RequestDropped,
    RequestRejected,
    RequestCancelled,

    // Performance measurements
    LatencyMeasured {
        duration_ms: u64,
        class: LatencyClass,
    },

    // State transitions
    DegradedModeChanged {
        old: DegradedMode,
        new: DegradedMode,
    },

    // Gauges
    QueueDepthUpdated {
        depth: usize,
    },
    ActiveSessionsUpdated {
        active: usize,
    },
    MemoryUpdated {
        rss_bytes: u64,
    },
}

#[derive(Debug, Clone)]
pub enum LatencyClass {
    QueueWait,
    FirstToken,
    TotalGeneration,
}

pub trait MetricSink: Send + Sync {
    fn record(&self, event: MetricEvent);
}

pub fn dispatch(event: MetricEvent) {
    // 1. Prometheus Sink
    crate::metrics::prometheus_sink::PrometheusSink.record(event.clone());

    // 2. OpenTelemetry Sink (Coming next)
    // crate::metrics::telemetry::record(event);
}
