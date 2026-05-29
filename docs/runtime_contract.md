# HYPERCORE Runtime Contract

This document defines the strict, unchangeable semantics of the HYPERCORE inference runtime. Internal code may change; these behavioral contracts **must not**.

## 1. Single Architecture Boundary
```text
HTTP / CLI
    ↓
[ tokio async boundary ]
    ↓
[ bounded request queue ]
    ↓
[ spawn_blocking: engine run_loop ]
    ↓
[ synchronous inference ]
    ↓
[ bounded token sender channel ]
    ↓
HTTP / CLI
```
**Rule**: There is exactly one async boundary around inference. The core engine (`LlamaEngine`) operates entirely synchronously in a dedicated blocking thread to eliminate scheduler jitter, hidden deadlocks, and cancellation leaks.

## 2. Invariants & Ownership
*   **Absolute Authority**: `SafetyGovernor` is the *only* component allowed to make admission, downgrade, or rejection decisions. The API layer, CLI, and engine validators MUST remain "dumb transports" without any adaptive logic.
*   **Panic Intolerance**: The engine traps panics inside its outer boundary and maps them to `RuntimeFailure::EngineFault`. A panic in the decoder must *never* crash the overarching runtime.
*   **Stateful Hysteresis**: The governor degrades memory pressure immediately but demands a delayed, monotonically safe window before recovering.

## 3. Queue & Cancellation Semantics
*   **Queue Boundedness**: The request queue is strictly bounded. If it hits capacity, new requests are rejected instantly.
*   **Cancellations**: Client disconnects or explicit cancellations drop the token stream. The engine guarantees termination of the compute loop within bounded time T. The orchestrator must not "ghost generate."
*   **Drop Policy**: During `CriticalMemoryPressure`, low-priority requests are aggressively dropped. Dropped requests return `RuntimeFailure::AdmissionRejected`.

## 4. Channel Semantics
*   The token return channel (`response_tx`) uses a bounded capacity.
*   **Slow Consumers**: If the channel is full, tokens are dropped to maintain engine throughput. HYPERCORE prioritizes system stability and engine liveness over perfect zero-loss guarantees for extremely slow clients.

## 5. Non-Goals
HYPERCORE intentionally does **NOT** guarantee:
*   **Perfect latency under saturation**: The system protects memory and liveness first; tail latency will degrade under heavy queuing.
*   **Zero token loss for slow consumers**: Disconnected or stalling subscribers will be pruned.
*   **Fairness across all request classes**: High-priority jobs will systematically starve low-priority jobs under critical pressure.
*   **Infinite queue durability**: Excess load is explicitly rejected, not enqueued forever.
