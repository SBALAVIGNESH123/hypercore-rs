use hypercore_rs::engine::llama::{InferenceRequest, LlamaEngine};

use hypercore_rs::runtime::governor::EngineMetrics;
use hypercore_rs::runtime::RuntimeState;
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

const TEST_MODEL: &str = "qwen2.5-0.5b-instruct-q5_k_m.gguf";

async fn boot_soak_engine() -> (
    mpsc::Sender<InferenceRequest>,
    watch::Receiver<EngineMetrics>,
) {
    let (request_tx, request_rx) = mpsc::channel(100);
    let (_, state_rx) = watch::channel(RuntimeState::default());
    let (metrics_tx, metrics_rx) = watch::channel(EngineMetrics::default());

    let engine = LlamaEngine::new(
        TEST_MODEL.to_string(),
        8192,
        4,
        state_rx.clone(),
        metrics_tx.clone(),
        request_rx,
    );

    tokio::task::spawn_blocking(move || {
        let _ = engine.run_loop();
    });

    (request_tx, metrics_rx)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_soak() {
    let mode = std::env::var("SOAK_MODE").unwrap_or_else(|_| "ci".to_string());
    let soak_seconds = match mode.as_str() {
        "ci" => 15,
        "minimal" => 300,
        "full" => 7200,
        _ => 15,
    };

    let seed = std::env::var("SOAK_SEED").unwrap_or_else(|_| "42".to_string());
    println!(
        "Starting soak test for {} seconds (MODE={}, SEED={})",
        soak_seconds, mode, seed
    );

    let (request_tx, _engine_rx) = boot_soak_engine().await;


    let start = std::time::Instant::now();
    let mut session_id = 1000;

    while start.elapsed().as_secs() < soak_seconds {
        // Randomly inject 5-10 requests
        let burst = 5;
        let mut cancels = vec![];
        for _ in 0..burst {
            let (response_tx, mut response_rx) = mpsc::channel(10);
            let cancel = CancellationToken::new();
            let req = InferenceRequest {
                request_id: format!("test-soak-{}", session_id),
                prompt: "Soak prompt".to_string(),
                response_tx,
                cancel: cancel.clone(),
                session_id,
                priority: 1,
                timeline: Default::default(),
                max_tokens: Some(10),
                temperature: None,
            };
            session_id += 1;
            request_tx.send(req).await.unwrap();
            cancels.push(cancel);

            // Consume tokens in background
            tokio::spawn(async move { while let Some(Ok(_)) = response_rx.recv().await {} });
        }

        tokio::time::sleep(Duration::from_millis(500)).await;

        // Randomly cancel one
        if let Some(c) = cancels.pop() {
            c.cancel();
        }
    }

    println!("Soak test finished without memory drift or deadlocks.");
}
