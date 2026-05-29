# Changelog

All notable changes to Hypercore will be documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0] - 2026-05-29

### Features
- **OpenAI Compatible API:** Full support for `/v1/chat/completions` and `/v1/models`.
- **Continuous Batching:** Round-robin slot scheduler enabling multiple concurrent streams with zero queue stalling.
- **Streaming:** Server-Sent Events (SSE) support natively out-of-the-box.
- **Safety Governor:** Automatic state transitions (`Running` -> `Throttled` -> `Paused`) based on real-time memory and swap pressure.
- **System Watchdog:** Background thread samples physical memory usage every 250ms via `sysinfo`.
- **Prometheus Metrics:** Exporting queue depth, token rates, and latency breakdowns on `/metrics`.
- **3-Stage Graceful Shutdown:** Ensures in-flight inference tasks complete before process exit (Drain -> Timeout -> Exit).

### Engine Behavior
- Supports arbitrary `.gguf` weights on CPU (tested with Llama 3, Mistral, and Qwen architectures).
- Implements strict RAII guards on KV-cache slots to prevent memory exhaustion under high concurrency.
- Monotonic atomic counters for request and session identifiers.

### API Stability
- Reached feature complete stability for the `1.0.0` surface area.
- Bearer token authentication via `HYPERCORE_API_KEY` is fully enforced.
- ChatML and OpenAI Chat schemas are fully supported for validation.

### Known Limitations
- GPU offloading is not currently enabled for the release binaries (CPU-first focus).
- DeepSeek reasoning tags (`<think>`) are not natively stripped from the output stream yet.
- Max theoretical batch size is hardcoded to engine constraints and requires manual tuning via `hypercore.yaml`.

---
*Hypercore is built for predictable reliability.*
