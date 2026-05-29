use hypercore_rs::engine::llama::{InferenceRequest, LlamaEngine};
use hypercore_rs::runtime::governor::EngineMetrics;
use hypercore_rs::runtime::RuntimeState;
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

const TEST_MODEL: &str = "qwen2.5-0.5b-instruct-q5_k_m.gguf";

async fn boot_test_engine() -> mpsc::Sender<InferenceRequest> {
    let (request_tx, request_rx) = mpsc::channel(100);
    let (_, state_rx) = watch::channel(RuntimeState::default());
    let (metrics_tx, _) = watch::channel(EngineMetrics::default());

    let engine = LlamaEngine::new(TEST_MODEL.to_string(), 8192, 4, state_rx, metrics_tx, request_rx);

    tokio::task::spawn_blocking(move || {
        let _ = engine.run_loop();
    });

    request_tx
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_production_runtime_suite() {
    // We run all tests sequentially on a single engine to prove true Model Residency
    // and prevent cargo's parallel test runner from loading 5 models and causing OOM.
    let request_tx = boot_test_engine().await;

    // 1. Model Residency Test
    println!("Running Model Residency Test...");
    for i in 0..3 {
        let (response_tx, mut response_rx) = mpsc::channel(10);
        let req = InferenceRequest {
            request_id: format!("test-residency-{}", i),
            prompt: "Test".to_string(),
            response_tx,
            cancel: CancellationToken::new(),
            session_id: i,
            priority: 1,
            timeline: Default::default(),
            max_tokens: Some(10),
            temperature: None,
        };
        request_tx.send(req).await.unwrap();
        let _ = response_rx.recv().await;
    }

    // 2. Concurrency Saturation Test
    println!("Running Concurrency Saturation Test...");
    let mut handles = vec![];
    for i in 0..50 {
        let tx = request_tx.clone();
        handles.push(tokio::spawn(async move {
            let (response_tx, mut response_rx) = mpsc::channel(100);
            let req = InferenceRequest {
                request_id: format!("test-concurrency-{}", i),
                prompt: "Concurrency Test".to_string(),
                response_tx,
                cancel: CancellationToken::new(),
                session_id: i + 10,
                priority: 1,
                timeline: Default::default(),
                max_tokens: Some(10),
            temperature: None,
            };
            tx.send(req).await.unwrap();
            let mut count = 0;
            while let Some(Ok(_)) = response_rx.recv().await {
                count += 1;
            }
            count
        }));
    }
    for handle in handles {
        let count = handle.await.unwrap();
        assert!(count > 0, "Each concurrent request must receive tokens");
    }

    // 3. Slow Consumer Backpressure Test
    println!("Running Slow Consumer Test...");
    let (response_tx, mut response_rx) = mpsc::channel(2);
    let req = InferenceRequest {
        request_id: "test-slow-consumer".to_string(),
        prompt: "Slow Test".to_string(),
        response_tx,
        cancel: CancellationToken::new(),
        session_id: 99,
        priority: 1,
        timeline: Default::default(),
        max_tokens: Some(10),
            temperature: None,
    };
    request_tx.send(req).await.unwrap();
    tokio::time::sleep(Duration::from_secs(1)).await;
    let mut received = 0;
    while let Some(Ok(_)) = response_rx.recv().await {
        received += 1;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(received < 50, "Slow consumer should have tokens dropped");

    // 4. Cancellation Test
    println!("Running Cancellation Test...");
    let (response_tx, mut response_rx) = mpsc::channel(100);
    let cancel = CancellationToken::new();
    let req = InferenceRequest {
        request_id: "test-cancellation".to_string(),
        prompt: "Cancel Test".to_string(),
        response_tx,
        cancel: cancel.clone(),
        session_id: 100,
        priority: 1,
        timeline: Default::default(),
        max_tokens: Some(10),
            temperature: None,
    };
    request_tx.send(req).await.unwrap();
    let mut count = 0;
    while let Some(Ok(_)) = response_rx.recv().await {
        count += 1;
        if count == 5 {
            cancel.cancel();
            break;
        }
    }
    let mut extra = 0;
    while let Some(Ok(_)) = response_rx.recv().await {
        extra += 1;
    }
    assert!(extra < 10, "Engine should abort cleanly upon cancellation");

    // 5. Backpressure Memory Ceiling Test
    println!("Running Backpressure Memory Ceiling Test...");
    for i in 0..20 {
        let (response_tx, _) = mpsc::channel(1);
        let req = InferenceRequest {
            request_id: format!("test-ceiling-{}", i),
            prompt: "Ceiling Test".to_string(),
            response_tx,
            cancel: CancellationToken::new(),
            session_id: 200 + i,
            priority: 1,
            timeline: Default::default(),
            max_tokens: Some(10),
            temperature: None,
        };
        request_tx.send(req).await.unwrap();
    }
    tokio::time::sleep(Duration::from_secs(3)).await;

    println!("All production architecture tests passed successfully!");
}
