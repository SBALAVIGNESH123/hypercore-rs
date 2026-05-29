use hypercore_rs::core::error::RuntimeFailure;
use hypercore_rs::core::state::RequestTimeline;
use hypercore_rs::engine::llama::{InferenceRequest, InferenceResponse, LlamaEngine};
use hypercore_rs::metrics::SystemMetrics;
use hypercore_rs::runtime::governor::{EngineMetrics, SafetyGovernor};
use hypercore_rs::runtime::RuntimeState;
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

const TEST_MODEL: &str = "qwen2.5-0.5b-instruct-q5_k_m.gguf";

async fn boot_test_engine() -> (
    mpsc::Sender<InferenceRequest>,
    watch::Receiver<EngineMetrics>,
    watch::Sender<SystemMetrics>,
) {
    let (request_tx, request_rx) = mpsc::channel(100);
    let (state_tx, state_rx) = watch::channel(RuntimeState::default());
    let (metrics_tx, _engine_rx) = watch::channel(EngineMetrics::default());
    let (sys_tx, sys_rx) = watch::channel(SystemMetrics::default());

    let engine = LlamaEngine::new(
        TEST_MODEL.to_string(),
        8192,
        4,
        state_rx.clone(),
        metrics_tx.clone(),
        request_rx,
    );

    let governor = SafetyGovernor::new(sys_rx, metrics_tx.subscribe(), state_tx);
    tokio::spawn(governor.run());

    tokio::task::spawn_blocking(move || {
        let _ = engine.run_loop();
    });

    // Give the engine time to load the model before returning
    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

    (request_tx, metrics_tx.subscribe(), sys_tx)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_priority_inversion() {
    let (request_tx, _engine_rx, _sys_tx) = boot_test_engine().await;

    // 1. Launch Low Priority
    let (low_tx, mut low_rx) = mpsc::channel(10);
    let low_req = InferenceRequest {
        request_id: "test-low-priority".to_string(),
        prompt: "Low priority task".to_string(),
        response_tx: low_tx,
        cancel: CancellationToken::new(),
        session_id: 1,
        priority: 2,
        timeline: Default::default(),
        max_tokens: Some(10),
        temperature: None,
    };
    request_tx.send(low_req).await.unwrap();

    // 2. Flood with High Priority
    for i in 0..5 {
        let (tx, mut hi_rx) = mpsc::channel::<Result<InferenceResponse, RuntimeFailure>>(100);
        let hi_req = InferenceRequest {
            request_id: format!("test-hi-priority-{}", i),
            prompt: "High priority task".to_string(),
            response_tx: tx,
            cancel: CancellationToken::new(),
            session_id: 1000 + i,
            priority: 0,
            timeline: Default::default(),
            max_tokens: Some(10),
            temperature: None,
        };
        request_tx.send(hi_req).await.unwrap();

        tokio::spawn(async move { while let Some(Ok(_)) = hi_rx.recv().await {} });
    }

    // 3. Ensure Low Priority still completes (Bounded Starvation)
    let res = tokio::time::timeout(Duration::from_secs(15), async move {
        let mut count = 0;
        while let Some(Ok(_)) = low_rx.recv().await {
            count += 1;
            if count > 5 {
                break;
            }
        }
    })
    .await;

    assert!(res.is_ok(), "Low priority task was completely starved!");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_allocator_churn() {
    let (request_tx, _engine_rx, sys_tx) = boot_test_engine().await;

    let mut peak_rss_violations = 0;

    for i in 0..50 {
        // Force high memory pressure artificially to test degradation
        if i % 10 == 0 {
            let sys = SystemMetrics {
                memory_pressure_pct: 85.0,
                ..Default::default()
            };
            let _ = sys_tx.send(sys);
        } else {
            let _ = sys_tx.send(SystemMetrics::default());
        }

        let (resp_tx, mut resp_rx) = mpsc::channel(10);
        let req = InferenceRequest {
            request_id: format!("test-churn-{}", i),
            prompt: format!(
                "Churn test prompt length variance {} {} {}",
                i,
                i * 2,
                i * 3
            ),
            response_tx: resp_tx,
            cancel: CancellationToken::new(),
            session_id: 1000 + (i as usize),
            priority: 1,
            timeline: RequestTimeline::default(),
            max_tokens: Some(10),
            temperature: None,
        };
        let _ = request_tx.send(req).await;

        while let Some(Ok(_)) = resp_rx.recv().await {}

        // Bounded queue: measure if RSS is trending unsafely
        let sys = sys_tx.borrow().clone();
        if sys.memory_pressure_pct > 95.0 {
            peak_rss_violations += 1;
        }
    }

    assert_eq!(
        peak_rss_violations, 0,
        "Memory churn caused unrecoverable ceiling violation!"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_graceful_saturation() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .try_init();
    let (request_tx, _engine_rx, sys_tx) = boot_test_engine().await;

    let sys = SystemMetrics {
        memory_pressure_pct: 98.0,
        ..Default::default()
    };
    let _ = sys_tx.send(sys);
    tokio::time::sleep(Duration::from_millis(200)).await; // Allow governor to react

    let mut rejected_count = 0;
    let mut admitted_count = 0;

    for i in 0..100 {
        let (resp_tx, mut resp_rx) = mpsc::channel(10);
        let priority = if i % 2 == 0 { 0 } else { 2 }; // Mix of high and low priority

        let req = InferenceRequest {
            request_id: format!("test-saturation-{}", i),
            prompt: "Saturation test".to_string(),
            response_tx: resp_tx,
            cancel: CancellationToken::new(),
            session_id: 2000 + i,
            priority,
            timeline: RequestTimeline::default(),
            max_tokens: Some(10),
            temperature: None,
        };

        let cancel_token = req.cancel.clone();

        request_tx.send(req).await.unwrap();

        match tokio::time::timeout(Duration::from_millis(500), resp_rx.recv()).await {
            Ok(Some(Err(_))) => {
                rejected_count += 1;
            }
            Ok(Some(Ok(InferenceResponse::Admitted))) => {
                admitted_count += 1;
                cancel_token.cancel(); // Cancel immediately so it doesn't hold up a slot
            }
            Ok(Some(Ok(InferenceResponse::Token(_)))) => {}
            Ok(None) => {}
            Err(_) => {
                // True timeout means engine deadlock or starvation
                panic!("Request {} timed out waiting for admission!", i);
            }
        }
    }

    println!(
        "Saturation: {} admitted, {} rejected",
        admitted_count, rejected_count
    );
    assert!(
        rejected_count > 0,
        "Failed to drop low-priority under critical load!"
    );
    assert!(
        admitted_count > 0,
        "Failed to admit high-priority under critical load!"
    );
}
