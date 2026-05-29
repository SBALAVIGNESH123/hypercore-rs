use crate::metrics::events::{MetricEvent, MetricSink, LatencyClass};
use lazy_static::lazy_static;
use prometheus::{Histogram, IntCounter, IntGauge, Registry};

lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();

    // Gauges
    pub static ref QUEUE_DEPTH: IntGauge = IntGauge::new("hypercore_queue_depth", "Current queue depth").unwrap();
    pub static ref ACTIVE_SESSIONS: IntGauge = IntGauge::new("hypercore_active_sessions", "Active inference sessions").unwrap();
    pub static ref MEMORY_RSS: IntGauge = IntGauge::new("hypercore_memory_rss_bytes", "Engine RSS Memory").unwrap();

    // Counters
    pub static ref REQUESTS_ENQUEUED: IntCounter = IntCounter::new("hypercore_requests_enqueued_total", "Total requests enqueued").unwrap();
    pub static ref REQUESTS_ADMITTED: IntCounter = IntCounter::new("hypercore_requests_admitted_total", "Total requests admitted to engine").unwrap();
    pub static ref TOKENS_GENERATED: IntCounter = IntCounter::new("hypercore_tokens_generated_total", "Total tokens generated").unwrap();
    pub static ref REQUESTS_DROPPED: IntCounter = IntCounter::new("hypercore_requests_dropped_total", "Total requests dropped due to queue full or pressure").unwrap();
    pub static ref REQUESTS_REJECTED: IntCounter = IntCounter::new("hypercore_requests_rejected_total", "Total requests rejected at admission").unwrap();
    pub static ref REQUESTS_CANCELLED: IntCounter = IntCounter::new("hypercore_requests_cancelled_total", "Total requests explicitly cancelled").unwrap();
    pub static ref DEGRADED_TRANSITIONS: IntCounter = IntCounter::new("hypercore_degraded_transitions_total", "Total transitions into degraded modes").unwrap();

    // Histograms
    pub static ref LATENCY_QUEUE: Histogram = Histogram::with_opts(prometheus::HistogramOpts::new("hypercore_latency_queue_ms", "Time spent in queue")).unwrap();
    pub static ref LATENCY_TTFT: Histogram = Histogram::with_opts(prometheus::HistogramOpts::new("hypercore_latency_ttft_ms", "Time to first token")).unwrap();
    pub static ref LATENCY_TOTAL: Histogram = Histogram::with_opts(prometheus::HistogramOpts::new("hypercore_latency_total_ms", "Total request generation time")).unwrap();
}

pub fn register_metrics() {
    REGISTRY.register(Box::new(QUEUE_DEPTH.clone())).unwrap();
    REGISTRY.register(Box::new(ACTIVE_SESSIONS.clone())).unwrap();
    REGISTRY.register(Box::new(MEMORY_RSS.clone())).unwrap();
    REGISTRY.register(Box::new(REQUESTS_ENQUEUED.clone())).unwrap();
    REGISTRY.register(Box::new(REQUESTS_ADMITTED.clone())).unwrap();
    REGISTRY.register(Box::new(TOKENS_GENERATED.clone())).unwrap();
    REGISTRY.register(Box::new(REQUESTS_DROPPED.clone())).unwrap();
    REGISTRY.register(Box::new(REQUESTS_REJECTED.clone())).unwrap();
    REGISTRY.register(Box::new(REQUESTS_CANCELLED.clone())).unwrap();
    REGISTRY.register(Box::new(DEGRADED_TRANSITIONS.clone())).unwrap();
    REGISTRY.register(Box::new(LATENCY_QUEUE.clone())).unwrap();
    REGISTRY.register(Box::new(LATENCY_TTFT.clone())).unwrap();
    REGISTRY.register(Box::new(LATENCY_TOTAL.clone())).unwrap();
}

pub struct PrometheusSink;

impl MetricSink for PrometheusSink {
    fn record(&self, event: MetricEvent) {
        match event {
            MetricEvent::RequestEnqueued => REQUESTS_ENQUEUED.inc(),
            MetricEvent::RequestAdmitted => REQUESTS_ADMITTED.inc(),
            MetricEvent::TokenGenerated { count } => TOKENS_GENERATED.inc_by(count as u64),
            MetricEvent::RequestDropped => REQUESTS_DROPPED.inc(),
            MetricEvent::RequestRejected => REQUESTS_REJECTED.inc(),
            MetricEvent::RequestCancelled => REQUESTS_CANCELLED.inc(),
            MetricEvent::LatencyMeasured { duration_ms, class } => match class {
                LatencyClass::QueueWait => LATENCY_QUEUE.observe(duration_ms as f64),
                LatencyClass::FirstToken => LATENCY_TTFT.observe(duration_ms as f64),
                LatencyClass::TotalGeneration => LATENCY_TOTAL.observe(duration_ms as f64),
            },
            MetricEvent::DegradedModeChanged { .. } => DEGRADED_TRANSITIONS.inc(),
            MetricEvent::QueueDepthUpdated { depth } => QUEUE_DEPTH.set(depth as i64),
            MetricEvent::ActiveSessionsUpdated { active } => ACTIVE_SESSIONS.set(active as i64),
            MetricEvent::MemoryUpdated { rss_bytes } => MEMORY_RSS.set(rss_bytes as i64),
        }
    }
}
