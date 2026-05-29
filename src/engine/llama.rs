use crate::core::error::RuntimeFailure;
use crate::core::state::{RequestState, RequestTimeline};
use crate::metrics::events::{dispatch, MetricEvent};
use crate::runtime::governor::EngineMetrics;
use crate::runtime::invariant::InvariantGuard;
use crate::runtime::{RuntimeMode, RuntimeState};
use anyhow::Result;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::LlamaModel;
use std::num::NonZeroU32;
use std::panic::{catch_unwind, AssertUnwindSafe};
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};


const MAX_ACTIVE_SESSIONS: usize = 4;

#[derive(Debug, Clone)]
pub enum InferenceResponse {
    Admitted,
    Token(String),
}

pub struct InferenceRequest {
    pub request_id: String,
    pub prompt: String,
    pub response_tx: mpsc::Sender<Result<InferenceResponse, RuntimeFailure>>,
    pub cancel: CancellationToken,
    pub session_id: usize,
    pub priority: u8, // 0 = High, 1 = Normal, 2 = Low
    pub timeline: RequestTimeline,
    pub max_tokens: Option<usize>,
    pub temperature: Option<f32>,
}

struct ActiveRequest {
    req: InferenceRequest,
    pending_tokens: Vec<llama_cpp_2::token::LlamaToken>,
    n_past: i32,
    generated: i32,
    tokens_to_generate: usize,
    _kv_guard: InvariantGuard,
    kv_slot: i32,
    deadline: std::time::Instant,
}

pub struct LlamaEngine {
    model_path: String,
    context_size: u32,
    n_threads: u32,
    state_rx: watch::Receiver<RuntimeState>,
    metrics_tx: watch::Sender<EngineMetrics>,
    request_rx: mpsc::Receiver<InferenceRequest>,
}

impl LlamaEngine {
    pub fn new(
        model_path: String,
        context_size: u32,
        n_threads: u32,
        state_rx: watch::Receiver<RuntimeState>,
        metrics_tx: watch::Sender<EngineMetrics>,
        request_rx: mpsc::Receiver<InferenceRequest>,
    ) -> Self {
        Self {
            model_path,
            context_size,
            n_threads,
            state_rx,
            metrics_tx,
            request_rx,
        }
    }

    pub fn run_loop(mut self) -> Result<(), RuntimeFailure> {
        let res = catch_unwind(AssertUnwindSafe(|| self.run_loop_inner()));
        match res {
            Ok(Ok(())) => {
                println!("LlamaEngine: Graceful shutdown Ok(())");
                Ok(())
            }
            Ok(Err(e)) => {
                println!("LlamaEngine: run_loop_inner returned Err({:?})", e);
                Err(e)
            }
            Err(payload) => {
                let msg = if let Some(s) = payload.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = payload.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "Unknown panic".to_string()
                };
                println!("LlamaEngine: PANICKED: {}", msg);
                Err(RuntimeFailure::EngineFault(format!(
                    "Engine Panicked: {}",
                    msg
                )))
            }
        }
    }

    fn run_loop_inner(&mut self) -> Result<(), RuntimeFailure> {
        info!("LlamaEngine: Initializing llama.cpp backend...");
        let backend = LlamaBackend::init()
            .map_err(|_| RuntimeFailure::EngineFault("Failed to initialize backend".to_string()))?;

        let model_params = LlamaModelParams::default().with_n_gpu_layers(0);
        info!("LlamaEngine: Loading model from {}...", self.model_path);

        if !std::path::Path::new(&self.model_path).exists() {
            return Err(RuntimeFailure::ModelFault(format!(
                "Model path not found: {}",
                self.model_path
            )));
        }

        let model = LlamaModel::load_from_file(&backend, &self.model_path, &model_params)
            .map_err(|e| RuntimeFailure::ModelFault(e.to_string()))?;

        // Use config-driven context size and thread count
        let n_ctx = self.context_size;
        let n_threads = self.n_threads;
        info!("LlamaEngine: n_ctx={}, n_threads={}", n_ctx, n_threads);
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(n_ctx))
            .with_n_threads(n_threads as i32)
            .with_n_seq_max(MAX_ACTIVE_SESSIONS as u32)
            .with_n_threads_batch(n_threads as i32);

        let mut ctx = model
            .new_context(&backend, ctx_params)
            .map_err(|_| RuntimeFailure::EngineFault("Failed to create context".to_string()))?;

        info!("LlamaEngine: Resident model loaded, waiting for jobs...");

        let mut active_requests: Vec<ActiveRequest> = Vec::new();
        let mut batch = llama_cpp_2::llama_batch::LlamaBatch::new(1024, 1);

        // Request timeout: 120 seconds per request
        let request_timeout = std::time::Duration::from_secs(120);

        loop {
            // 0. Process Cancellations first so we don't deadlock if Paused
            let mut i = 0;
            while i < active_requests.len() {
                if active_requests[i].req.cancel.is_cancelled() {
                    let ar = &mut active_requests[i];
                    info!("[Session {}] Cancelled.", ar.req.session_id);
                    ar.req.timeline.transition(RequestState::Cancelled);
                    Self::dump_timeline(&ar.req);
                    let _ = ctx.clear_kv_cache_seq(Some(ar.kv_slot as u32), None, None);
                    active_requests.remove(i);
                    continue;
                }
                i += 1;
            }

            // 1. Fill available slots up to MAX_ACTIVE_SESSIONS
            while active_requests.len() < MAX_ACTIVE_SESSIONS {
                let req_opt = if active_requests.is_empty() {
                    self.request_rx.blocking_recv()
                } else {
                    self.request_rx.try_recv().ok()
                };

                if let Some(mut req) = req_opt {
                    let state = self.state_rx.borrow().clone();
                    let tokens_to_generate = req.max_tokens.unwrap_or(50);

                    let max_ctx = self.context_size as usize;

                    // 1. Hard bound on max_tokens
                    if tokens_to_generate > max_ctx {
                        warn!("[Session {}] Rejected: max_tokens {} exceeds {} limit.", req.session_id, tokens_to_generate, max_ctx);
                        req.timeline.transition(RequestState::Rejected);
                        let _ = req.response_tx.try_send(Err(RuntimeFailure::AdmissionRejected(format!("max_tokens exceeds context limit of {}", max_ctx))));
                        Self::dump_timeline(&req);
                        continue;
                    }

                    let tokens = model.str_to_token(&req.prompt, llama_cpp_2::model::AddBos::Always).unwrap_or_else(|_| vec![llama_cpp_2::token::LlamaToken(1)]);

                    // 2. Early Context Length Rejection
                    if tokens.len() + tokens_to_generate > max_ctx {
                        warn!("[Session {}] Rejected: prompt ({}) + max_tokens ({}) exceeds {} context limit.", req.session_id, tokens.len(), tokens_to_generate, max_ctx);
                        req.timeline.transition(RequestState::Rejected);
                        let _ = req.response_tx.try_send(Err(RuntimeFailure::AdmissionRejected(format!("Prompt length + max_tokens exceeds context limit of {}", max_ctx))));
                        Self::dump_timeline(&req);
                        continue;
                    }

                    // Apply admission rules based on governor state
                    // ISSUE-1 FIX: Reject explicitly under memory pressure instead of silently clamping
                    let mut rejected = false;
                    match state.degraded_mode {
                        crate::runtime::DegradedMode::Healthy => {}
                        crate::runtime::DegradedMode::MemoryPressure => {
                            if req.priority > 0 {
                                warn!("[Session {}] MemoryPressure: Rejecting low-priority request.", req.session_id);
                                req.timeline.transition(RequestState::Rejected);
                                let _ = req.response_tx.try_send(Err(RuntimeFailure::MemoryPressure));
                                Self::dump_timeline(&req);
                                rejected = true;
                            }
                            // High-priority (0) requests are still admitted at full token budget
                        }
                        crate::runtime::DegradedMode::CriticalMemoryPressure => {
                            warn!("[Session {}] CriticalMemoryPressure: Rejecting all requests.", req.session_id);
                            req.timeline.transition(RequestState::Rejected);
                            let _ = req.response_tx.try_send(Err(RuntimeFailure::MemoryPressure));
                            Self::dump_timeline(&req);
                            rejected = true;
                        }
                    }

                    if !rejected {
                        req.timeline.transition(RequestState::Admitted);
                        let _ = req.response_tx.try_send(Ok(InferenceResponse::Admitted));
                        
                        let mut kv_slot = -1;
                        for slot in 0..(MAX_ACTIVE_SESSIONS as i32) {
                            if !active_requests.iter().any(|ar| ar.kv_slot == slot) {
                                kv_slot = slot;
                                break;
                            }
                        }
                        
                        active_requests.push(ActiveRequest {
                            _kv_guard: InvariantGuard::acquire_kv_cache(req.session_id as u64),
                            req,
                            pending_tokens: tokens,
                            n_past: 0,
                            generated: 0,
                            tokens_to_generate,
                            kv_slot,
                            deadline: std::time::Instant::now() + request_timeout,
                        });
                        info!("BatchScheduler: Admitted session. Active: {}/{}", active_requests.len(), MAX_ACTIVE_SESSIONS);
                    }
                } else {
                    break;
                }
            }

            if active_requests.is_empty() {
                println!("LlamaEngine: Exiting loop because active_requests is empty and channel closed.");
                break; // Queue is closed and all work is done
            }

            // 2. Timeout enforcement
            let now_instant = std::time::Instant::now();
            let mut ti = 0;
            while ti < active_requests.len() {
                if now_instant >= active_requests[ti].deadline {
                    let ar = &mut active_requests[ti];
                    warn!("[Session {}] TIMEOUT after 120s.", ar.req.session_id);
                    ar.req.timeline.transition(RequestState::Failed);
                    let _ = ar.req.response_tx.try_send(Err(RuntimeFailure::Timeout("Request exceeded 120s timeout".into())));
                    Self::dump_timeline(&ar.req);
                    let _ = ctx.clear_kv_cache_seq(Some(ar.kv_slot as u32), None, None);
                    active_requests.remove(ti);
                    continue;
                }
                ti += 1;
            }

            // 3. State & Throttle Check
            let state = self.state_rx.borrow().clone();
            match state.mode {
                RuntimeMode::Paused => {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    continue;
                }
                RuntimeMode::Throttled => {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                RuntimeMode::Running => {}
            }

            // 3. Build Batch from Active Requests
            batch.clear();
            let mut batch_indices = Vec::new();
            
            let chunk_size = 256;
            let max_batch = 1024;
            let mut added_this_step = 0;
            let mut made_progress = true;

            // Round-robin chunked prefill & decode
            while made_progress && added_this_step < max_batch {
                made_progress = false;
                for (req_idx, ar) in active_requests.iter_mut().enumerate() {
                    if ar.pending_tokens.is_empty() || added_this_step >= max_batch {
                        continue;
                    }

                    let take_count = std::cmp::min(chunk_size, ar.pending_tokens.len());
                    let take_count = std::cmp::min(take_count, max_batch - added_this_step);

                    let mut last_idx = -1;
                    for j in 0..take_count {
                        let token = ar.pending_tokens[j];
                        // Request logits only for the final token of the full sequence
                        let is_last = j == ar.pending_tokens.len() - 1;
                        let idx = batch.n_tokens();
                        
                        if let Err(e) = batch.add(token, ar.n_past, &[ar.kv_slot], is_last) {
                            tracing::error!("Batch add failed: {:?}", e);
                            break;
                        }
                        ar.n_past += 1;
                        added_this_step += 1;
                        made_progress = true;

                        if is_last {
                            last_idx = idx;
                        }
                    }

                    ar.pending_tokens.drain(0..take_count);

                    if last_idx >= 0 {
                        batch_indices.push((req_idx, last_idx));
                    }
                }
            }

            if batch.n_tokens() == 0 {
                // tracing::debug!("Batch empty, spinning...");
                std::thread::sleep(std::time::Duration::from_millis(10));
                continue;
            }

            // 4. Decode
            let start_time = std::time::Instant::now();
            if let Err(e) = ctx.decode(&mut batch) {
                println!("LlamaEngine: Batch decode failed: {:?}", e);
                warn!("Batch decode failed: {:?}", e);
                // In a production system we'd handle OOM and evict here. For now we clear and continue.
                ctx.clear_kv_cache();
                break;
            }

            let elapsed = start_time.elapsed().as_secs_f32();
            let tps = if elapsed > 0.0 { batch.n_tokens() as f32 / elapsed } else { 0.0 };
            
            // ISSUE-2 FIX: Report actual active session count instead of hardcoded 0
            let active_count = active_requests.len();
            let _ = self.metrics_tx.send(EngineMetrics {
                tokens_per_sec: tps,
                queue_depth: active_count,
                stalled: false,
                source: crate::runtime::governor::MetricSource::LlamaEngine,
                timestamp: std::time::Instant::now(),
                latency_class: crate::runtime::governor::LatencyClass::Compute,
            });

            // ISSUE-3 FIX: Dispatch active sessions metric
            dispatch(MetricEvent::ActiveSessionsUpdated { active: active_count });

            // 5. Sample & Yield
            let mut i = 0;
            while i < active_requests.len() {
                if let Some((_, last_idx)) = batch_indices.iter().find(|(req_idx, _)| *req_idx == i) {
                    tracing::info!("[Session {}] Found batch index last_idx={}", active_requests[i].req.session_id, last_idx);
                    if *last_idx >= 0 {
                        let ar = &mut active_requests[i];

                        // Build per-request sampler based on temperature
                        let temp = ar.req.temperature.unwrap_or(0.0);
                        let mut sampler = if temp > 0.01 {
                            // Temperature-based sampling: temp scales logits, dist samples from distribution
                            llama_cpp_2::sampling::LlamaSampler::chain_simple([
                                llama_cpp_2::sampling::LlamaSampler::temp(temp),
                                llama_cpp_2::sampling::LlamaSampler::dist(0), // seed=0 for non-deterministic
                            ])
                        } else {
                            llama_cpp_2::sampling::LlamaSampler::greedy()
                        };

                        let next_token = sampler.sample(&ctx, *last_idx);
                        tracing::info!("[Session {}] Sampled token_id={}", ar.req.session_id, next_token.0);

                        // EOS detection: check if this token is end-of-generation
                        if model.is_eog_token(next_token) {
                            info!("[Session {}] EOS token detected. Completing.", ar.req.session_id);
                            ar.req.timeline.transition(RequestState::Completed);
                            Self::dump_timeline(&ar.req);
                            let _ = ctx.clear_kv_cache_seq(Some(ar.kv_slot as u32), None, None);
                            active_requests.remove(i);
                            continue;
                        }

                        ar.pending_tokens.push(next_token);

                        let token_bytes = model.token_to_piece_bytes(next_token, 64, false, None).unwrap_or_default();
                        let token_str = String::from_utf8_lossy(&token_bytes).into_owned();
                        tracing::info!("[Session {}] Sending token_str='{}'", ar.req.session_id, token_str);

                        match ar.req.response_tx.try_send(Ok(InferenceResponse::Token(token_str))) {
                            Ok(_) => {
                                if ar.generated == 0 {
                                    ar.req.timeline.transition(RequestState::Active);
                                }
                                ar.generated += 1;
                                dispatch(MetricEvent::TokenGenerated { count: 1 });
                            }
                            Err(mpsc::error::TrySendError::Closed(_)) => {
                                info!("[Session {}] Client dropped.", ar.req.session_id);
                                ar.req.timeline.transition(RequestState::Dropped);
                                Self::dump_timeline(&ar.req);
                                let _ = ctx.clear_kv_cache_seq(Some(ar.kv_slot as u32), None, None);
                                active_requests.remove(i);
                                continue;
                            }
                            Err(mpsc::error::TrySendError::Full(_)) => {
                                tracing::warn!("[Session {}] Channel FULL! Token dropped.", ar.req.session_id);
                            }
                        }

                        if ar.generated as usize >= ar.tokens_to_generate {
                            info!("[Session {}] Completed.", ar.req.session_id);
                            ar.req.timeline.transition(RequestState::Completed);
                            Self::dump_timeline(&ar.req);
                            let _ = ctx.clear_kv_cache_seq(Some(ar.kv_slot as u32), None, None);
                            active_requests.remove(i);
                            continue;
                        }
                    }
                }
                i += 1;
            }
        }

        info!("LlamaEngine: Shutting down run loop.");
        Ok(())
    }

    fn dump_timeline(req: &InferenceRequest) {
        if let Ok(json) = serde_json::to_string(&req.timeline) {
            info!("[Session {}] TIMELINE: {}", req.session_id, json);
        }
        crate::metrics::telemetry::export_timeline(req.session_id as u64, &req.timeline);
    }
}
