use crate::engine::llama::{InferenceRequest, InferenceResponse};
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tracing::info;

pub struct SessionStats {
    pub session_id: u64,
    pub qsd_ms: f64,
    pub ttft_ms: f64,
    pub avg_itl_ms: f64,
    pub token_count: usize,
    pub duration_s: f64,
}

pub async fn run_benchmark(
    model_path: &str,
    concurrency: usize,
    max_tokens: usize,
    request_tx: mpsc::Sender<InferenceRequest>,
) -> anyhow::Result<()> {
    info!("Starting HYPERCORE Benchmark...");
    info!("Model: {}", model_path);
    info!("Concurrency: {}", concurrency);
    info!("Target tokens per session: {}", max_tokens);

    let (stats_tx, mut stats_rx) = mpsc::channel(concurrency);
    let global_start = Instant::now();

    for i in 0..concurrency {
        let (response_tx, mut response_rx) = mpsc::channel(100);
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();
        let session_id = 999000 + i as u64;
        let req_tx = request_tx.clone();
        let stats_tx = stats_tx.clone();
        let tokens = max_tokens;

        tokio::spawn(async move {
            let req = InferenceRequest {
                request_id: uuid::Uuid::new_v4().to_string(),
                prompt: "Once upon a time in a distant galaxy".to_string(),
                response_tx,
                cancel: cancel_clone.clone(),
                session_id: session_id as usize,
                priority: 1,
                timeline: Default::default(),
                max_tokens: Some(tokens),
                temperature: None,
            };

            let t0_submission = Instant::now();
            let mut t_admitted = None;
            let mut t_first_token = None;
            let mut itls = Vec::new();
            let mut last_token_time = None;
            let mut token_count = 0;

            if let Err(e) = req_tx.send(req).await {
                tracing::error!("Failed to send benchmark job {}: {:?}", session_id, e);
                return;
            }

            while let Some(res) = response_rx.recv().await {
                match res {
                    Ok(InferenceResponse::Admitted) => {
                        t_admitted = Some(Instant::now());
                    }
                    Ok(InferenceResponse::Token(_)) => {
                        let now = Instant::now();
                        token_count += 1;

                        if t_first_token.is_none() {
                            t_first_token = Some(now);
                        } else if let Some(last) = last_token_time {
                            itls.push(now.duration_since(last).as_secs_f64() * 1000.0);
                        }
                        
                        last_token_time = Some(now);

                        if token_count >= max_tokens {
                            cancel_clone.cancel();
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::error!("Session {} Engine Error: {:?}", session_id, e);
                        break;
                    }
                }
            }

            let qsd_ms = t_admitted.map(|t| t.duration_since(t0_submission).as_secs_f64() * 1000.0).unwrap_or(0.0);
            let ttft_ms = t_first_token.map(|t| t.duration_since(t0_submission).as_secs_f64() * 1000.0).unwrap_or(0.0);
            let avg_itl_ms = if itls.is_empty() { 0.0 } else { itls.iter().sum::<f64>() / itls.len() as f64 };
            let duration_s = t0_submission.elapsed().as_secs_f64();

            let _ = stats_tx.send(SessionStats {
                session_id,
                qsd_ms,
                ttft_ms,
                avg_itl_ms,
                token_count,
                duration_s,
            }).await;
        });
    }

    // Drop the original sender so the receiver can finish when all clones drop
    drop(stats_tx);

    let mut all_stats = Vec::new();
    let mut total_tokens = 0;

    while let Some(stat) = stats_rx.recv().await {
        total_tokens += stat.token_count;
        all_stats.push(stat);
    }

    let global_elapsed = global_start.elapsed().as_secs_f64();
    let global_tps = if global_elapsed > 0.0 {
        total_tokens as f64 / global_elapsed
    } else {
        0.0
    };

    let avg_qsd = if !all_stats.is_empty() { all_stats.iter().map(|s| s.qsd_ms).sum::<f64>() / all_stats.len() as f64 } else { 0.0 };
    let avg_ttft = if !all_stats.is_empty() { all_stats.iter().map(|s| s.ttft_ms).sum::<f64>() / all_stats.len() as f64 } else { 0.0 };
    let avg_itl = if !all_stats.is_empty() { all_stats.iter().map(|s| s.avg_itl_ms).sum::<f64>() / all_stats.len() as f64 } else { 0.0 };

    info!("======================================");
    info!("BENCHMARK RESULTS ({} Sessions)", concurrency);
    info!("Total Wall Time : {:.2} s", global_elapsed);
    info!("Aggregate TPS   : {:.2} tokens/sec", global_tps);
    info!("Total Tokens    : {}", total_tokens);
    info!("--------------------------------------");
    info!("Avg QSD         : {:.2} ms (Queue Saturation Delay)", avg_qsd);
    info!("Avg TTFT        : {:.2} ms (Time to First Token)", avg_ttft);
    info!("Avg ITL         : {:.2} ms/token (Inter-Token Latency)", avg_itl);
    info!("======================================");

    Ok(())
}
