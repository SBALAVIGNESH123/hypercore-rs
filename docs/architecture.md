# Hypercore Architecture

Hypercore is built on a clear separation of concerns, designed specifically for deterministic execution and reliability. 

## High-Level Architecture

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

## Request Lifecycle

The lifecycle of every inference request is strictly tracked and managed:

```
Client → API (auth, CORS, validation)
       → Queue (backpressure, drain check)
       → Engine (tokenize → validate → admit/reject)
       → Batch Scheduler (round-robin, KV-slot allocation)
       → Sample (greedy or temp-based)
       → EOS check (stop or continue)
       → Token → API → Client (SSE or JSON)
```

## Internal Components

### 1. API Layer (Axum)
Handles all HTTP interactions. Validates inbound JSON payloads against the OpenAI schema. Applies backpressure (`429 Too Many Requests`) if the internal engine queue is saturated. Validates `Bearer` tokens via the Auth middleware.

### 2. Engine Layer (llama.cpp)
Manages the actual context windows. It handles round-robin scheduling for continuous batching. Slot allocation ensures that if we run out of KV-cache memory, new requests are rejected early instead of causing OOMs during generation.

### 3. Safety Governor & Watchdog
A background task (`watchdog`) samples system metrics every 250ms using `sysinfo`. It monitors physical RAM and swap space. If memory pressure exceeds the defined `memory_limit_mb`, the `Governor` forces a state transition to `Throttled` or `Paused`, instructing the Engine to reject new requests until pressure subsides.

### 4. Telemetry
Every single request is tracked with a precise `RequestTimeline`:
- `queued_at`
- `admitted_at`
- `first_token_at`
- `completed_at`

This allows operators to distinguish between "queue wait time" and actual "generation time" when debugging latency issues in Grafana.
