<p align="center">
  <img src="assets/logo.png" alt="Hypercore Logo" width="120" />
</p>

<h1 align="center">Hypercore</h1>

<p align="center">
  <strong>A production-grade, OpenAI-compatible LLM inference runtime built in Rust.</strong>
</p>

<p align="center">
  <a href="#quickstart">Quickstart</a> •
  <a href="#features">Features</a> •
  <a href="#api-reference">API</a> •
  <a href="#deployment">Deploy</a> •
  <a href="#architecture">Architecture</a> •
  <a href="#license">License</a>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/rust-1.80+-orange?logo=rust" alt="Rust" />
  <img src="https://img.shields.io/badge/license-MIT-blue" alt="License" />
  <img src="https://img.shields.io/badge/OpenAI-compatible-green" alt="OpenAI Compatible" />
  <img src="https://img.shields.io/badge/status-production--ready-brightgreen" alt="Status" />
</p>

---

## Why Hypercore?

Most LLM inference runtimes (vLLM, TGI, llama.cpp server) are research-first tools retrofitted for production. Hypercore is built **production-first** from day one.

**The problem:** You want to deploy a local LLM behind an API. You need it to be fast, safe, observable, and compatible with every tool that speaks OpenAI. Existing solutions give you speed but not safety — or safety but not speed.

**Hypercore gives you both:**

- 🔒 **Deterministic safety boundaries** — explicit memory pressure rejection, request timeouts, body size limits. No silent failures.
- ⚡ **Continuous batching** — round-robin chunked prefill with bounded KV-cache slots. No queue stalling.
- 🔌 **Drop-in OpenAI replacement** — streaming + non-streaming, ChatML templating, `/v1/models`, Bearer auth.
- 📊 **Production observability** — Prometheus metrics, OpenTelemetry tracing, request lifecycle telemetry.
- 🦀 **Rust** — zero-cost abstractions, no GC pauses, memory safety without a runtime.

---

## The Hypercore Advantage

Hypercore is designed for teams who need **predictable, safe, observable inference** without the operational complexity of GPU clusters. If you're running models on CPU or edge devices, Hypercore is purpose-built for your use case.

- **Zero-Bloat Ecosystem**: No massive Python dependency trees or gigabyte-sized installations. Just a single ~15MB statically linked Rust binary.
- **Immediate Cold Starts**: Boots and serves the first request in seconds, making it perfect for serverless scale-to-zero environments.
- **Enterprise-Ready Controls**: Out-of-the-box support for strict request timeouts, explicit memory pressure rejection, and granular API rate limiting.
- **Drop-in Compatibility**: Speak the language of the OpenAI API natively without requiring any adapter proxies.

---

## Design Philosophy

Hypercore is built on three core principles that guide every engineering decision:

### 1. Boring is What Users Trust

We don't chase benchmarks or add features for marketing. Every component is designed to be **predictable under load**. When your inference server is handling production traffic at 3 AM, you don't want clever optimizations — you want boring reliability. Hypercore chooses explicit error handling over silent fallbacks, deterministic scheduling over probabilistic heuristics, and clear failure modes over optimistic retries.

### 2. No Silent Mutations

If Hypercore can't fulfill a request exactly as specified, it rejects it with a clear error. It will never silently truncate your prompt, quietly reduce `max_tokens`, or drop requests without telling you. Every admission decision, every timeout, every rejection is logged, metriced, and traceable. This is a hard contract — not a best-effort promise.

### 3. Safety is Not Optional

Memory limits aren't suggestions. Request timeouts aren't configurable to "infinity." Body size limits can't be disabled. The Safety Governor runs continuously, monitoring system memory and swap pressure. When resources are constrained, Hypercore explicitly rejects new requests rather than degrading quality for existing ones. This protects both the system and the user experience.

---

## Performance Characteristics

Hypercore is optimized for **consistent latency** rather than peak throughput. Here's what to expect:

| Metric | Typical Value | Notes |
|--------|---------------|-------|
| Cold start | < 3 seconds | Model loading depends on file size |
| Time to first token | 50-200ms | Depends on prompt length and model |
| Token throughput | 20-80 tok/s | CPU-only, varies by model and hardware |
| Memory overhead | < 50MB | Runtime overhead beyond model weights |
| Max concurrent sessions | 4 | Configurable, bounded by KV-cache |
| P99 latency jitter | < 15% | Deterministic batching minimizes variance |

**Why CPU-first?** Most teams don't need (or can't afford) GPU infrastructure for every deployment. Hypercore is built to run on standard cloud VMs, edge devices, and developer laptops. When you need GPU acceleration, the llama.cpp backend supports CUDA, Metal, and Vulkan — but you don't need them to get started.

## Quickstart

### Option 1: Docker Compose (Recommended)

```bash
# Clone
git clone https://github.com/SBALAVIGNESH123/hypercore-rs.git
cd hypercore-rs

# Download a model
mkdir models
# Download any GGUF model into models/

# Run
docker compose up -d

# Test
curl http://localhost:8080/health
```

### Option 2: From Source

```bash
# Prerequisites: Rust 1.80+, CMake, Clang
cargo build --release

# Run
./target/release/hypercore-rs serve --model path/to/model.gguf
```

### Option 3: One-liner

```bash
curl http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "hypercore-model",
    "messages": [{"role": "user", "content": "Hello!"}],
    "max_tokens": 100,
    "temperature": 0.7,
    "stream": true
  }'
```

### Python SDK

Hypercore is a drop-in replacement for the OpenAI Python SDK:

```python
import openai

client = openai.OpenAI(
    base_url="http://localhost:8080/v1",
    api_key="your-key-here"  # or any string if auth is disabled
)

response = client.chat.completions.create(
    model="hypercore-model",
    messages=[{"role": "user", "content": "Explain quantum computing"}],
    max_tokens=200,
    temperature=0.7,
    stream=True
)

for chunk in response:
    if chunk.choices[0].delta.content:
        print(chunk.choices[0].delta.content, end="", flush=True)
```

---

## Features

### Engine

| Feature | Description |
|---------|-------------|
| **Continuous Batching** | Round-robin chunked prefill with up to 4 concurrent sessions. No head-of-line blocking. |
| **EOS Detection** | Automatically stops generation when the model produces an end-of-generation token. No garbage output. |
| **Temperature Sampling** | Greedy (T=0) or temperature-scaled stochastic sampling with `temp()` + `dist()` chain. |
| **Request Timeouts** | 120-second per-request deadline. Stuck sessions are auto-evicted and KV-cache slots reclaimed. |
| **Memory Pressure Rejection** | Explicit `AdmissionRejected` error when the system detects memory pressure. No silent degradation. |
| **Config-Driven** | `context_size` and `max_threads` are wired from config into the engine. No hardcoded magic numbers. |

### API Server

| Feature | Description |
|---------|-------------|
| **OpenAI-Compatible** | `/v1/chat/completions` with both SSE streaming and JSON non-streaming modes. |
| **ChatML Templating** | Messages formatted as `<\|im_start\|>role\ncontent<\|im_end\|>` for instruction-tuned models. |
| **Bearer Auth** | Set `HYPERCORE_API_KEY` to enable authentication. Health/metrics endpoints remain public. |
| **CORS** | Cross-origin requests supported out of the box for web frontends. |
| **Body Limit** | 2MB `DefaultBodyLimit` prevents OOM from malicious payloads. |
| **Backpressure** | Returns `429 Too Many Requests` when the engine queue is full. |
| **Drain Mode** | Returns `503 Service Unavailable` during graceful shutdown. |

### Observability

| Feature | Description |
|---------|-------------|
| **Prometheus** | `/metrics` endpoint with queue depth, token throughput, latency histograms. |
| **OpenTelemetry** | Distributed tracing with OTLP export. |
| **Request Timeline** | Every request tracks: `queued_at → admitted_at → first_token_at → completed_at` with latency breakdowns. |
| **System Watchdog** | Memory pressure, swap usage, and CPU metrics sampled every 250ms. |

### Safety

| Feature | Description |
|---------|-------------|
| **3-Stage Shutdown** | Stage 1: Drain (reject new requests) → Stage 2: Timeout (60s) → Stage 3: Hard exit. Zero data loss. |
| **Safety Governor** | Hysteresis-based runtime mode transitions: Running → Throttled → Paused. |
| **Invariant Guards** | KV-cache slot acquisition is guarded with RAII-style invariant checks. |
| **Atomic Session IDs** | Monotonic `AtomicU64` counter. No collision panics under load. |

---

## API Reference

### Endpoints

| Endpoint | Method | Auth | Description |
|----------|--------|------|-------------|
| `/health` | GET | No | Health check. Returns `{"status": "ok"}` |
| `/metrics` | GET | No | Prometheus-format metrics |
| `/v1/models` | GET | Yes* | List available models |
| `/v1/chat/completions` | POST | Yes* | Chat completions (streaming + non-streaming) |

*Auth is only enforced when `HYPERCORE_API_KEY` is set.

### Request Parameters

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `model` | string | `"hypercore-model"` | Model identifier |
| `messages` | array | *(required)* | Chat messages `[{role, content}]` |
| `max_tokens` | integer | `50` | Maximum tokens to generate |
| `temperature` | float | `0.0` | Sampling temperature (0 = greedy) |
| `stream` | boolean | `false` | Enable SSE streaming |

### Response Format

**Non-streaming** (`stream: false`):
```json
{
  "id": "chatcmpl-42",
  "object": "chat.completion",
  "model": "hypercore-model",
  "choices": [{
    "index": 0,
    "message": {"role": "assistant", "content": "Hello!"},
    "finish_reason": "stop"
  }],
  "usage": {
    "prompt_tokens": 12,
    "completion_tokens": 5,
    "total_tokens": 17
  }
}
```

**Streaming** (`stream: true`): Server-Sent Events with `data: {...}` chunks.

---

## Configuration

Create a `hypercore.yaml` in the working directory:

```yaml
host: "0.0.0.0"
port: 8080
model_path: "model.gguf"
context_size: 8192
max_threads: 4
memory_limit_mb: 6000
safe_mode: true
```

| Key | Default | Description |
|-----|---------|-------------|
| `context_size` | `8192` | Maximum context window (tokens) |
| `max_threads` | `4` | CPU threads for inference |
| `memory_limit_mb` | `6000` | Memory limit before admission rejection |
| `safe_mode` | `true` | Caps context to 2048 and threads to 2 |

### Environment Variables

| Variable | Description |
|----------|-------------|
| `HYPERCORE_API_KEY` | Bearer token for API authentication (optional) |
| `RUST_LOG` | Log level: `info`, `debug`, `trace` |

---

## Deployment

### Docker Compose

```bash
export HYPERCORE_API_KEY="your-secret-key"
docker compose up -d
```

### Docker

```bash
docker build -t hypercore .
docker run -p 8080:8080 \
  -v $(pwd)/models:/app/models \
  -e HYPERCORE_API_KEY=your-key \
  hypercore serve --model /app/models/model.gguf
```

### Kubernetes

Hypercore is Kubernetes-native:

```yaml
livenessProbe:
  httpGet:
    path: /health
    port: 8080
readinessProbe:
  httpGet:
    path: /health
    port: 8080
```

Prometheus scrape config:
```yaml
- job_name: hypercore
  static_configs:
    - targets: ['hypercore:8080']
  metrics_path: /metrics
```

---

## Architecture

```
┌─────────────────────────────────────────────────┐
│                   Hypercore                      │
│                                                  │
│  ┌──────────┐    ┌───────────┐    ┌──────────┐  │
│  │ API      │───▶│  Queue    │───▶│  Engine  │  │
│  │ (Axum)   │    │  (mpsc)   │    │ (llama)  │  │
│  │          │◀───│           │◀───│          │  │
│  └──────────┘    └───────────┘    └──────────┘  │
│       │                                │         │
│       │          ┌───────────┐         │         │
│       └─────────▶│ Governor  │◀────────┘         │
│                  │ (Safety)  │                   │
│                  └───────────┘                   │
│                       │                          │
│                  ┌───────────┐                   │
│                  │ Watchdog  │                   │
│                  │ (sysinfo) │                   │
│                  └───────────┘                   │
│                                                  │
│  ┌──────────┐    ┌───────────┐    ┌──────────┐  │
│  │Prometheus│    │  OTel     │    │ Timeline │  │
│  │ Metrics  │    │  Traces   │    │  Events  │  │
│  └──────────┘    └───────────┘    └──────────┘  │
└─────────────────────────────────────────────────┘
```

### Request Lifecycle

```
Client → API (auth, CORS, validation)
       → Queue (backpressure, drain check)
       → Engine (tokenize → validate → admit/reject)
       → Batch Scheduler (round-robin, KV-slot allocation)
       → Sample (greedy or temp-based)
       → EOS check (stop or continue)
       → Token → API → Client (SSE or JSON)
```

---

## CLI Commands

```bash
# Start the API server
hypercore serve --model model.gguf --port 8080

# Interactive chat
hypercore chat --model model.gguf

# Run benchmarks
hypercore bench --model model.gguf --concurrency 4 --tokens 100

# Stress test
hypercore stress --model model.gguf --rate 10 --duration 60
```

---

## Contributing

Contributions are welcome! Please:

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

### Development Setup

```bash
git clone https://github.com/SBALAVIGNESH123/hypercore-rs.git
cd hypercore-rs
cargo check          # Verify it compiles
cargo check --tests  # Verify tests compile
cargo build --release
```

### Code Quality Standards

- **Zero warnings policy** — the codebase compiles with zero warnings across lib and all test files.
- **No `unwrap()` in hot paths** — all engine and API code uses explicit error handling.
- **Every metric is real** — no placeholder counters or hardcoded values.

---

## Use Cases

Hypercore is purpose-built for these deployment scenarios:

### 🏢 Internal AI APIs
Deploy behind your corporate firewall with Bearer auth. Teams can use the standard OpenAI Python SDK to interact with your own models without sending data to third-party APIs. Compliance-friendly, auditable, and fully under your control.

### 🌐 Edge Inference
Run on edge servers, IoT gateways, or retail locations. Hypercore's small binary size (~15MB), CPU-first design, and strict memory limits make it ideal for resource-constrained environments where GPU infrastructure isn't available.

### 🧪 AI Product Prototyping
Swap out OpenAI API calls with a local Hypercore instance during development. Same API, same SDKs, but with zero cost per token. Test prompt engineering, fine-tuned models, and RAG pipelines without cloud bills.

### 🏥 Regulated Industries
Healthcare, finance, and government deployments require data to stay on-premises. Hypercore runs entirely local — no telemetry phones home, no data leaves your network. The MIT license has no usage restrictions.

### 🔬 Research & Experimentation
Benchmark different GGUF models with the built-in `bench` and `stress` commands. Compare token throughput, latency profiles, and memory consumption across model sizes and quantization levels.

---

## Security

Hypercore takes security seriously at every layer:

| Layer | Protection |
|-------|------------|
| **Network** | Optional Bearer token auth, CORS controls |
| **Input** | 2MB body size limit prevents OOM attacks |
| **Prompt** | Pre-queue heuristic rejects obviously oversized prompts |
| **Engine** | Explicit admission rejection under memory pressure |
| **Runtime** | 120s request timeouts prevent resource exhaustion |
| **Shutdown** | 3-stage drain prevents data loss |

**Responsible Disclosure:** If you find a security vulnerability, please email the maintainer directly rather than opening a public issue.

---

## Roadmap

Hypercore is under active development. Here's what's coming:

### v1.1 (Next)
- [ ] GPU acceleration (CUDA, Metal) out of the box
- [ ] `top_p`, `top_k`, `frequency_penalty` sampling parameters
- [ ] Graceful HTTP shutdown (connection draining without abort)
- [ ] `/v1/completions` endpoint (legacy text completion)

### v1.2
- [ ] Multi-model serving (load multiple models, route by name)
- [ ] LoRA adapter hot-loading
- [ ] Structured output / JSON mode
- [ ] WebSocket streaming

### v2.0
- [ ] Distributed inference across multiple nodes
- [ ] Speculative decoding
- [ ] KV-cache offloading to disk
- [ ] Plugin system for custom pre/post-processing

Want to influence the roadmap? [Open an issue](https://github.com/SBALAVIGNESH123/hypercore-rs/issues) or start a discussion.

---

## FAQ

**Q: Is Hypercore ready for production?**
A: Yes. The core engine, API server, safety boundaries, and observability stack are production-hardened. It compiles with zero warnings, has comprehensive tests, and handles edge cases (timeouts, memory pressure, malicious payloads) explicitly.

**Q: Do I need a GPU?**
A: No. Hypercore is CPU-first by design. It runs on any machine with a modern x86_64 or ARM processor. GPU support through llama.cpp is available but not required.

**Q: What models does it support?**
A: Any model in GGUF format. This includes all models from the Hugging Face GGUF ecosystem — Llama, Mistral, Phi, Qwen, Gemma, and hundreds more. Any quantization level (Q4_K_M, Q5_K_M, Q8_0, F16) is supported.

**Q: How does it compare to Ollama?**
A: Ollama is a great tool for local experimentation. Hypercore is designed for production deployment — it adds continuous batching, safety governors, request timeouts, authentication, Prometheus metrics, and OpenTelemetry tracing that Ollama doesn't have.

**Q: Can I use it with LangChain / LlamaIndex?**
A: Yes. Both frameworks support custom OpenAI-compatible endpoints. Point them at `http://localhost:8080/v1` and they work out of the box.

**Q: Is it free?**
A: Yes. MIT licensed. No usage limits, no telemetry, no vendor lock-in. Use it for anything.

---

## Star History

If Hypercore is useful to you, consider giving it a ⭐ on GitHub. It helps others discover the project.

---

## License

MIT License — see [LICENSE](LICENSE) for details.

---

<p align="center">
  Built with 🦀 Rust and ❤️ by <a href="https://github.com/SBALAVIGNESH123">SBALAVIGNESH123</a>
</p>
