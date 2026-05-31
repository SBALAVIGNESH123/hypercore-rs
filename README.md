# Hypercore

**A local AI that learns from your documents, remembers your decisions, and helps you discover patterns in your own thinking.**

CPU-first LLM inference runtime + personal intelligence system, written in Rust.

![Rust](https://img.shields.io/badge/Rust-1.80+-orange) ![License](https://img.shields.io/badge/License-MIT-blue) ![OpenAI Compatible](https://img.shields.io/badge/API-OpenAI_Compatible-green) ![Release](https://img.shields.io/github/v/release/SBALAVIGNESH123/hypercore-rs)

[Quickstart](#-quickstart) · [Personal Intelligence](#-personal-intelligence) · [Inference Engine](#-inference-engine) · [Architecture](#-architecture) · [Contributing](#-contributing)

---

## The Problem

Every local AI tool today does the same thing: **store your documents, let you ask questions about them.**

That's retrieval. It's useful. But it's not intelligence.

Hypercore goes further. It reads your journals, meeting notes, architecture decisions, and project logs — then **tells you things about yourself you hadn't noticed.**

> *"Across 8 projects, you repeatedly chose simpler local deployments over more scalable architectures. You consistently optimize for independence rather than maximum scale."*

That's not search. That's synthesis. That's the difference.

---

## 🚀 Quickstart

### Install from Source

```bash
# Prerequisites: Rust 1.80+, CMake, Clang
git clone https://github.com/SBALAVIGNESH123/hypercore-rs.git
cd hypercore-rs
cargo build --release
```

### Your First Memory Graph

Three commands. That's all it takes.

```bash
# 1. Feed it your documents
hypercore ingest --path ./my-notes

# 2. See what it learned about you
hypercore memory show

# 3. Ask "Why am I like this?"
hypercore memory --model ./model.gguf explain
```

Hypercore ships with sample documents in `examples/` so you can try it immediately:

```bash
hypercore ingest --path ./examples
hypercore memory show
hypercore memory timeline
```

### Run as an API Server

Hypercore is also a full OpenAI-compatible inference server:

```bash
hypercore serve --model ./model.gguf --port 8080

curl http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model":"hypercore","messages":[{"role":"user","content":"Hello"}]}'
```

Drop-in replacement for the OpenAI SDK. Your existing code just works.

---

## 🧠 Personal Intelligence

### How It Works

Most AI tools stop at retrieval. Hypercore builds a **personal memory graph** from your documents and uses it to generate insights about your decision-making patterns.

```
Your Documents
    ↓
Ingestion (streaming, batched embeddings)
    ↓
SQLite Knowledge Store (FTS5 + vector search + dedup)
    ↓
Memory Extraction (natural language only — code files skipped)
    ↓
Memory Graph (Decisions · Preferences · Projects · Relationships)
    ↓
LLM Synthesis (compressed memories → local model → insight)
    ↓
Feedback ("Was this surprising?" → persisted 1-4 rating)
```

The entire pipeline runs locally. Your documents never leave your machine.

### Commands

| Command | What It Does |
|---|---|
| `memory show` | Display your extracted memory graph by category |
| `memory timeline` | Chronological view showing how your work evolved |
| `memory recall <topic>` | Find evidence and context for past decisions |
| `memory patterns` | Theme distribution, recurring words, source analysis |
| `memory explain` | **"Why am I like this?"** — synthesize your decision-making DNA |
| `memory insight` | Generate a weekly personal observation report |
| `memory sync <path>` | Sync a directory into the memory bank |

### With LLM Synthesis

Add `--model <path.gguf>` to unlock AI-powered synthesis:

```bash
# Without model → raw data + statistics
hypercore memory patterns

# With model → compressed memories → LLM → synthesized insight
hypercore memory --model ./qwen-3b.gguf patterns
```

The system automatically compresses your memories to fit within the model's context window, prints token budget instrumentation, and streams the response in real time.

### Feedback Loop

After every insight, Hypercore asks one question:

```
How valuable was this insight?
  1) Obvious — I already knew this
  2) Somewhat useful — mild interest
  3) Surprising — I hadn't noticed this
  4) Changed how I think — genuinely new perspective
```

Every rating is persisted to SQLite. This is how we measure whether the system produces real value — not retrieval accuracy, not benchmark scores, but **genuine user reactions.**

If nobody ever picks 3 or 4, we have work to do. If several users consistently pick 4, we've found something rare.

---

## ⚡ Inference Engine

Hypercore's inference runtime is production-grade, not a wrapper. It handles multi-session batching, memory pressure, and graceful degradation — the boring reliability that makes local AI actually usable.

### Core Capabilities

| Feature | Description |
|---|---|
| **Continuous Batching** | Round-robin chunked prefill with up to 4 concurrent sessions |
| **Memory Pressure Rejection** | Explicit `AdmissionRejected` under pressure — never silent OOMs |
| **Request Timeouts** | 300s deadline with auto-eviction of stuck sessions |
| **Temperature Sampling** | Greedy (T=0) or temperature-scaled stochastic sampling |
| **EOS Detection** | Auto-stop on end-of-generation tokens |
| **LoRA Support** | Adapter path configurable (loading not yet implemented — we say so honestly) |

### TitanMem

Our adaptive KV-cache congestion controller, built on real control theory:

- **Dual-signal EMA** — tracks both utilization and memory pressure
- **Hysteresis mode transitions** — Calm → Cautious → Critical with deadband to prevent flapping
- **Dynamic threshold tuning** — adapts to workload patterns over time
- **Per-session byte tracking** — fine-grained memory accounting

We built TitanMem, benchmarked it rigorously, and discovered it didn't outperform the OS page cache. **We published the data anyway** because honest engineering matters more than marketing. [Read the benchmarks →](docs/titanmem_benchmarks.md)

### API Server

| Endpoint | Method | Description |
|---|---|---|
| `/health` | GET | Health check |
| `/metrics` | GET | Prometheus-format metrics |
| `/v1/models` | GET | List available models |
| `/v1/chat/completions` | POST | Chat completions (streaming + non-streaming) |

OpenAI SDK compatible. Set `HYPERCORE_API_KEY` for bearer token authentication.

---

## 📊 Knowledge Store

The knowledge layer combines multiple retrieval strategies in a single SQLite database:

- **Hybrid search** — FTS5 full-text search + cosine vector similarity, automatically routed
- **Content-hash deduplication** — re-ingesting the same file is a no-op
- **Tree-sitter parsing** — function-level chunking for C/C++ source files
- **Streaming ingestion** — batch embedding (64 chunks per batch) with real-time progress
- **Smart filtering** — memory extraction only processes natural language files; source code is skipped entirely

---

## 📈 Evaluation

We don't hardcode benchmark scores. Every evaluation runs real retrieval:

```bash
hypercore studio eval my_assistant.yaml
```

```
Eval Results: my_assistant.yaml
  Questions:          4
  Retrieval Hits:     1 / 4
  Retrieval Accuracy: 25.0%
  Avg Top Score:      0.2318
  ✓ What are my most important projects? (score: 0.196, themes: ["HyperCore"])
  ✗ How do I typically write technical documents? (score: 0.240, themes: [])
  ✗ What decisions have I repeatedly made? (score: 0.172, themes: [])
  ✗ What technologies do I consistently prefer? (score: 0.318, themes: [])
```

25% accuracy. Not 92%. Because that's the real number.

---

## 🏗 Architecture

### Design Principles

**1. Boring is what users trust.**
Every component is designed to be predictable under load. We choose explicit error handling over silent fallbacks, deterministic scheduling over probabilistic heuristics, and clear failure modes over optimistic retries.

**2. No silent mutations.**
If Hypercore can't fulfill a request exactly as specified, it rejects it with a clear error. It will never silently truncate your prompt, quietly reduce max_tokens, or drop requests without telling you.

**3. Measure before you claim.**
Every performance claim is backed by reproducible benchmarks. If a subsystem doesn't demonstrate an advantage under rigorous testing, we say so. See: TitanMem.

**4. Insights over retrieval.**
The goal isn't "chat with your PDFs." It's "discover patterns in your thinking." That requires synthesis, not search.

### Benchmarks

Measured on AMD Ryzen 9 7900X, DDR5, with a 0.5B Q5_K_M GGUF model:

| Metric | Value |
|---|---|
| Binary Size | 15.8 MB |
| Idle RAM | ~45 MB |
| Cold Start | < 2.5s |
| Time to First Token | 55–120ms |
| Throughput (1 session) | ~45 tok/s |
| Throughput (4 sessions) | ~110 tok/s |

---

## ⚙️ Configuration

```yaml
# hypercore.yaml
host: "0.0.0.0"
port: 8080
model_path: "model.gguf"
context_size: 8192
max_threads: 4
memory_limit_mb: 6000
safe_mode: true
```

| Variable | Description |
|---|---|
| `HYPERCORE_API_KEY` | Bearer token for API authentication (optional) |
| `RUST_LOG` | Log level: `info`, `debug`, `trace` |

---

## 📋 Project Status

We believe in radical transparency. Here's exactly where every component stands:

| Component | Status | Notes |
|---|---|---|
| Inference Engine | ✅ Production-ready | Continuous batching, pressure handling, timeouts |
| TitanMem | ✅ Working | Experimental — honest benchmarks published |
| Knowledge Store | ✅ Working | FTS5 + vector search + dedup |
| Ingestion Pipeline | ✅ Working | Streaming, tree-sitter, batch embedding |
| Memory Extraction | ✅ Working | Keyword heuristics, code files skipped |
| Memory Commands | ✅ 7 commands | show, timeline, recall, patterns, explain, insight, sync |
| Feedback Collection | ✅ Working | 1–4 ratings persisted to SQLite |
| LLM Synthesis | 🔌 Wired | Needs GGUF model to test |
| Eval Pipeline | ✅ Real retrieval | No hardcoded scores |
| LoRA Fine-tuning | ❌ Not yet | Honest warning in code |

---

## 🤝 Contributing

We welcome contributions. Here's how:

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

If you're not sure where to start, check the [issues](https://github.com/SBALAVIGNESH123/hypercore-rs/issues).

---

## 📄 License

MIT License — see [LICENSE](LICENSE) for details.

---

<p align="center">
  Built with 🦀 Rust and ❤️ by <a href="https://github.com/SBALAVIGNESH123">Bala Vignesh S</a>
  <br><br>
  <a href="https://github.com/SBALAVIGNESH123/hypercore-rs/stargazers">⭐ Star us on GitHub</a> · <a href="https://github.com/SBALAVIGNESH123/hypercore-rs/releases">📦 Latest Release</a> · <a href="https://github.com/SBALAVIGNESH123/hypercore-rs/issues">🐛 Report a Bug</a>
</p>
