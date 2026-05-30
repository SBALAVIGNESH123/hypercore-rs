<p align="center">
  <img src="assets/logo.png" alt="Hypercore Logo" width="120" />
</p>

<h1 align="center">Hypercore</h1>

<p align="center">
  <strong>CPU-first LLM inference runtime for local AI ownership.</strong>
</p>

<p align="center">
  <a href="#quickstart">Quickstart</a> •
  <a href="#system-capabilities">Capabilities</a> •
  <a href="#benchmarks">Benchmarks</a> •
  <a href="#titanmem">TitanMem</a> •
  <a href="#limitations">Limitations</a> •
  <a href="#deployment">Deploy</a>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/rust-1.80+-orange?logo=rust" alt="Rust" />
  <img src="https://img.shields.io/badge/license-MIT-blue" alt="License" />
  <img src="https://img.shields.io/badge/OpenAI-compatible-green" alt="OpenAI Compatible" />
</p>

---

Hypercore is an OpenAI-compatible LLM inference server written in Rust, designed for internal APIs, edge inference, and on-prem deployments where reliability matters more than raw throughput.

- **Continuous batching** (up to 4 sessions)
- **Hard memory + context bounds** (no silent OOMs)
- **CPU-first**, single ~15MB binary
- **Prometheus + OpenTelemetry** built in
- **Drop-in replacement** for OpenAI SDK

---

## Quickstart

### Option 1: Docker Compose

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

### Option 3: Quick Test

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

---

## System Capabilities

### Engine

| Capability | Description |
|---------|-------------|
| **Continuous Batching** | Round-robin chunked prefill with up to 4 concurrent sessions. |
| **EOS Detection** | Automatically stops generation on end-of-generation tokens. |
| **Temperature Sampling** | Greedy (T=0) or temperature-scaled stochastic sampling. |
| **Request Timeouts** | 120-second per-request deadline. Stuck sessions are auto-evicted. |
| **Memory Pressure Rejection** | Explicit `AdmissionRejected` error when the system detects memory pressure. |

### API Server

| Capability | Description |
|---------|-------------|
| **OpenAI-Compatible** | `/v1/chat/completions` with both SSE streaming and JSON non-streaming modes. |
| **ChatML Templating** | Messages formatted natively for instruction-tuned models. |
| **Bearer Auth** | Set `HYPERCORE_API_KEY` to enable authentication. |
| **Body Limit** | 2MB `DefaultBodyLimit` prevents OOM from malicious payloads. |
| **Backpressure** | Returns `429 Too Many Requests` when the engine queue is saturated. |

### Observability

| Capability | Description |
|---------|-------------|
| **Prometheus** | `/metrics` endpoint with queue depth, token throughput, latency histograms. |
| **OpenTelemetry** | Distributed tracing with OTLP export. |
| **System Watchdog** | Memory pressure and CPU metrics sampled every 250ms. |

---

## Benchmarks

Measured on reference hardware (AMD Ryzen 9 7900X, DDR5) with a 0.5B Q5_K_M GGUF model.

*Results vary based on model size, quantization, and CPU architecture.*

| Metric | Value |
|--------|-------|
| **Binary Size** | `15.8 MB` |
| **Idle RAM Overhead** | `~45 MB` |
| **Cold Start** | `< 2.5s` |
| **Time To First Token** | `55ms - 120ms` |
| **Throughput (1 session)** | `~45 tokens/sec` |
| **Throughput (4 sessions)**| `~110 tokens/sec` |

---

## TitanMem

TitanMem is Hypercore's experimental memory virtualization subsystem. It was designed to improve inference speed when running models larger than available physical RAM.

**Status: Experimental — current implementation does not outperform native OS paging.**

We built TitanMem, benchmarked it rigorously under enforced memory pressure (via `SetProcessWorkingSetSizeEx` hard working set limits), and discovered that the Windows kernel's native demand paging already handles mmap'd model files near-optimally. Our prefetch strategy actively increased page faults and reduced throughput.

We published the data anyway because honest engineering matters more than marketing.

👉 **[Full benchmark results and methodology](docs/titanmem_benchmarks.md)**

### Key Finding

| Budget | Baseline Tok/s | TitanMem Tok/s | Baseline Page Faults | TitanMem Page Faults |
|--------|---------------|---------------|---------------------|---------------------|
| 1024 MB | **0.81** | 0.87 | 5,695,894 | 5,738,633 |
| 2048 MB | **1.92** | 1.54 | 1,594,375 | 1,765,599 |

TitanMem v1 is archived. Research continues into layer-aware scheduling and custom block I/O approaches.

### Reproduce

```bash
cargo build --release
python benchmarks/titanmem/run_blind_benchmarks.py
```

---

## Limitations

- **CPU inference only** (GPU support experimental via llama.cpp backend)
- **Max concurrency is bounded** (default: 4 sessions)
- **Not optimized for high-throughput public LLM APIs**
- **Best suited for internal / edge / controlled environments**
- **TitanMem memory engine is experimental and does not yet demonstrate an advantage**

---

## Architecture

Hypercore enforces strict lifecycle tracking, invariant assertions on KV-cache slot allocation, and proactive memory pressure monitoring. 

👉 **[Read the Architecture Document](docs/architecture.md)**

---

## Deployment Examples

Hypercore is designed to run anywhere, from single-node edge devices to Kubernetes clusters.

### 1. Docker Compose
```yaml
version: '3.8'
services:
  hypercore:
    image: ghcr.io/sbalavignesh123/hypercore-rs:v1.0.0
    ports:
      - "8080:8080"
    volumes:
      - ./models:/app/models
    environment:
      - HYPERCORE_API_KEY=sk-your-secure-key
    command: ["serve", "--model", "/app/models/model.gguf"]
```

### 2. systemd Service
```ini
[Unit]
Description=Hypercore LLM Inference Runtime
After=network.target

[Service]
ExecStart=/usr/local/bin/hypercore serve --model /var/lib/models/llama3.gguf
Restart=always
User=hypercore
Environment="RUST_LOG=info"

[Install]
WantedBy=multi-user.target
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
```

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

### Environment Variables

| Variable | Description |
|----------|-------------|
| `HYPERCORE_API_KEY` | Bearer token for API authentication (optional) |
| `RUST_LOG` | Log level: `info`, `debug`, `trace` |

---

## Design Philosophy

### 1. Boring is What Users Trust
Every component is designed to be **predictable under load**. Hypercore chooses explicit error handling over silent fallbacks, deterministic scheduling over probabilistic heuristics, and clear failure modes over optimistic retries.

### 2. No Silent Mutations
If Hypercore can't fulfill a request exactly as specified, it rejects it with a clear error. It will never silently truncate your prompt, quietly reduce `max_tokens`, or drop requests without telling you.

### 3. Safety is Not Optional
Memory limits aren't suggestions. Request timeouts aren't configurable to "infinity." Body size limits can't be disabled.

### 4. Measure Before You Claim
Every performance claim in this repository is backed by reproducible benchmarks. If a subsystem doesn't demonstrate an advantage under rigorous testing, we say so.

---

## Contributing

Contributions are welcome! Please:

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

---

## Security

| Layer | Protection |
|-------|------------|
| **Network** | Optional Bearer token auth, CORS controls |
| **Input** | 2MB body size limit prevents OOM attacks |
| **Prompt** | Pre-queue heuristic rejects obviously oversized prompts |
| **Engine** | Explicit admission rejection under memory pressure |
| **Runtime** | 120s request timeouts prevent resource exhaustion |
| **Shutdown** | 3-stage drain prevents data loss |

---

## License

MIT License — see [LICENSE](LICENSE) for details.

---

<p align="center">
  Built with 🦀 Rust and ❤️ by <a href="https://github.com/SBALAVIGNESH123">SBALAVIGNESH123</a>
</p>
