use crate::core::state::RequestTimeline;
use crate::engine::llama::{InferenceRequest, InferenceResponse};
use crate::metrics::stats::StatsAggregator;
use anyhow::Result;
use rand::Rng;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use rand_distr::{Distribution, Exp};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::info;

pub async fn run_stress(
    model: &str,
    rate: f64,
    burst_factor: f64,
    cancellation_prob: f64,
    duration_secs: u64,
    request_tx: mpsc::Sender<InferenceRequest>,
) -> Result<()> {
    info!(
        "Initializing Stochastic Stress Test (Poisson Arrivals) for model: {}",
        model
    );
    info!(
        "Params: Rate = {} req/s, Burst = {}, Cancel Prob = {}, Duration = {}s",
        rate, burst_factor, cancellation_prob, duration_secs
    );

    let stats = Arc::new(Mutex::new(StatsAggregator::new()));
    let start_time = Instant::now();
    let end_time = start_time + Duration::from_secs(duration_secs);

    let (cancel_tx, mut cancel_rx) = mpsc::channel::<(usize, CancellationToken)>(100);

    // We use an exponential distribution for inter-arrival times (Poisson process)
    let lambda = rate * burst_factor;
    let exp_dist = Exp::new(lambda).unwrap();
    let mut rng = ChaCha8Rng::seed_from_u64(42); // Deterministic seed for reproducible statistical noise

    let mut session_counter = 10000;

    // Background task to randomly cancel requests
    let stats_clone_cancel = stats.clone();
    tokio::spawn(async move {
        let mut rng = ChaCha8Rng::seed_from_u64(99);
        while let Some((_id, token)) = cancel_rx.recv().await {
            let p: f64 = rng.gen();
            if p < cancellation_prob {
                let delay = rng.gen_range(500..3000);
                tokio::time::sleep(Duration::from_millis(delay)).await;
                token.cancel();
                let mut s = stats_clone_cancel.lock().unwrap();
                s.cancelled_requests += 1;
                // Wait time is tracked approximately by events, but we just bump the counter
            }
        }
    });

    let mut wait_tasks = vec![];

    while Instant::now() < end_time {
        let inter_arrival_time = exp_dist.sample(&mut rng);
        tokio::time::sleep(Duration::from_secs_f64(inter_arrival_time)).await;

        if Instant::now() >= end_time {
            break;
        }

        // Generate prompt variance
        let p_type: f64 = rng.gen();
        let (prompt, max_tokens) = if p_type < 0.3 {
            ("What is 2+2?".to_string(), 20) // Small
        } else if p_type < 0.8 {
            (
                "Write a short paragraph about the history of artificial intelligence.".to_string(),
                100,
            ) // Medium
        } else {
            ("Write a detailed, comprehensive essay analyzing the geopolitical impact of the industrial revolution, including secondary effects on modern supply chains.".to_string(), 300)
            // Large
        };

        let token = CancellationToken::new();
        let (resp_tx, mut resp_rx) = mpsc::channel(100);

        let req = InferenceRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            prompt,
            response_tx: resp_tx,
            cancel: token.clone(),
            session_id: session_counter,
            priority: 1,
            timeline: RequestTimeline::default(),
            max_tokens: Some(max_tokens),
            temperature: None,
        };

        session_counter += 1;

        {
            let mut s = stats.lock().unwrap();
            s.total_requests += 1;
            s.record_queue_depth(request_tx.max_capacity() - request_tx.capacity());
            // Track how many items are in the channel
        }

        if request_tx.send(req).await.is_err() {
            break;
        }

        let _ = cancel_tx.send((session_counter - 1, token)).await;

        let stats_clone = stats.clone();
        let start_req = Instant::now();

        let handle = tokio::spawn(async move {
            let mut ttft = 0;
            let mut last_token_time = None;

            while let Some(res) = resp_rx.recv().await {
                match res {
                    Ok(InferenceResponse::Admitted) => {
                        // Admission received
                    }
                    Ok(InferenceResponse::Token(_)) => {
                        let now = Instant::now();
                        if ttft == 0 {
                            ttft = start_req.elapsed().as_millis() as u64;
                            stats_clone.lock().unwrap().record_ttft(ttft);
                        }
                        if let Some(lt) = last_token_time {
                            let itl = now.duration_since(lt).as_millis() as u64;
                            stats_clone.lock().unwrap().record_itl(itl);
                        }
                        last_token_time = Some(now);
                    }
                    Err(_) => {
                        let mut s = stats_clone.lock().unwrap();
                        s.rejected_requests += 1;
                        s.record_drop(start_req.elapsed().as_millis() as u64);
                        return;
                    }
                }
            }
            if ttft > 0 {
                let mut s = stats_clone.lock().unwrap();
                s.completed_requests += 1;
            }
        });

        wait_tasks.push(handle);
    }

    info!("Stress phase complete. Waiting up to 10 seconds for drain...");

    // Wait for remaining tasks with timeout
    let _ = tokio::time::timeout(Duration::from_secs(10), async {
        for handle in wait_tasks {
            let _ = handle.await;
        }
    })
    .await;

    let mut final_stats = stats.lock().unwrap();
    final_stats.duration_ms = start_time.elapsed().as_millis() as u64;
    final_stats.print_report();

    Ok(())
}
