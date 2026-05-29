# Boring Reliability: Design Tradeoffs in HYPERCORE

HYPERCORE is intentionally designed to be "boring" internally. It operates on the philosophy that **predictable degradation under hostile conditions** is significantly more valuable than theoretical fairness or maximum burst throughput.

This document explicitly outlines *why* the system behaves the way it does.

## 1. Why Drops are Preferable to Stalls

Under extreme memory pressure or saturation load, systems typically do one of two things:
1. **Stall**: Pause execution, hoping resources will free up.
2. **Drop**: Reject work immediately to protect the system.

**HYPERCORE's Choice**: We aggressively drop low-priority requests and heavily throttle high-priority requests.
**Why?** A stalled system is functionally indistinguishable from a dead system to the end user. Queuing indefinitely leads to cascading failures, exhausted client timeouts, and memory leaks. By dropping requests explicitly, we return control to the client immediately and mathematically bound the system's memory envelope, ensuring the engine *always* remains alive to process the next critical job.

## 2. Why Hysteresis Exists

Mathematical "pure function" thresholds (e.g., `if memory > 80% { Degraded } else { Healthy }`) fail in production because they cause **oscillation collapse**. 

If memory hits 80.1%, the system degrades and memory drops to 79.9%. The system recovers, accepts load, and instantly bounces back to 80.1%. This rapid switching destroys throughput and creates unstable, stuttering latency.

**HYPERCORE's Choice**: Stateful Hysteresis. Memory pressure triggers degradation *instantly* (protecting the system), but recovery requires a delayed, monotonically safe window (e.g., memory must drop below 75% and the derivative slope must be negative for N seconds). This creates calm, predictable modes instead of jitter.

## 3. Why Observability is Separated from Control

In many runtimes, telemetry metrics (like the p95 latency of the last 100 requests) are fed directly back into the admission controller to shape traffic.

**HYPERCORE's Choice**: Strict separation of tiers. 
- Tier 1 (Hard System Limits) controls admission.
- Tier 2 (Soft SLOs / Metrics) and Tier 3 (Logs/Traces) are *strictly observer-only*.

**Why?** If diagnostic paths affect runtime behavior, you create invisible feedback loops. A broken metrics agent or a temporary latency spike could accidentally trigger an admission collapse. By ensuring observers *never* mutate shared state or trigger retries, the core engine remains mathematically predictable regardless of what the telemetry layer observes.

## 4. Why Bounded Memory Beats Fairness

"Fairness" suggests that all requests should get a slice of compute and memory, executing round-robin.

**HYPERCORE's Choice**: Strict priority starvation. Under pressure, low-priority jobs are rejected to ensure high-priority jobs complete.

**Why?** Fairness under saturation leads to "context switching death." If 1000 requests try to share a 2GB KV cache, they all get 2MB, none of them can generate meaningful output, and all 1000 clients time out. By aggressively rejecting 990 requests and granting the cache exclusively to the top 10, the system successfully services 10 clients instead of 0. Predictable throughput beats theoretical fairness.
