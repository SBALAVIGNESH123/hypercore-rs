#![allow(clippy::await_holding_lock)]
#![allow(clippy::manual_range_contains)]
mod common;

use hypercore_rs::core::state::RequestTimeline;
use hypercore_rs::engine::llama::{InferenceRequest, InferenceResponse, LlamaEngine};
use hypercore_rs::runtime::{DegradedMode, RuntimeMode, RuntimeState};
use std::sync::Mutex;
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

lazy_static::lazy_static! {
    static ref TEST_MUTEX: Mutex<()> = Mutex::new(());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_concurrent_4_session_stability() {
    let _guard = TEST_MUTEX.lock().unwrap();
    let fixture = match common::get_fixture() {
        Some(f) => f,
        None => {
            println!("Skipping test: TEST_MODEL=download not set and fixture missing.");
            return;
        }
    };

    let (_state_tx, state_rx) = watch::channel(RuntimeState {
        mode: RuntimeMode::Running,
        degraded_mode: DegradedMode::Healthy,
        active_tokens: 0,
        max_tokens: 8192,
    });

    let (metrics_tx, _metrics_rx) =
        watch::channel(hypercore_rs::runtime::governor::EngineMetrics {
            tokens_per_sec: 0.0,
            queue_depth: 0,
            stalled: false,
            source: hypercore_rs::runtime::governor::MetricSource::LlamaEngine,
            timestamp: std::time::Instant::now(),
            latency_class: hypercore_rs::runtime::governor::LatencyClass::Compute,
        });

    let (req_tx, req_rx) = mpsc::channel(10);

    let engine = LlamaEngine::new(fixture, 8192, 4, state_rx, metrics_tx, req_rx);

    let engine_handle = std::thread::spawn(move || {
        engine.run_loop().expect("Engine panicked");
    });

    let mut responses = Vec::new();
    for i in 0..4 {
        let (resp_tx, mut resp_rx) = mpsc::channel(100);
        let req = InferenceRequest {
            request_id: format!("test-concurrent-{}", i),
            prompt: format!("Sequence {} data", i),
            response_tx: resp_tx,
            cancel: CancellationToken::new(),
            session_id: i as usize,
            priority: 1,
            timeline: RequestTimeline::default(),
            max_tokens: Some(10),
            temperature: None,
        };
        req_tx.send(req).await.unwrap();

        let handle = tokio::spawn(async move {
            let mut count = 0;
            while let Some(Ok(resp)) = resp_rx.recv().await {
                if let InferenceResponse::Token(_) = resp {
                    count += 1;
                }
            }
            count
        });
        responses.push(handle);
    }

    for handle in responses {
        let count = handle.await.unwrap();
        assert!(count > 0, "Expected at least 1 token generated per session");
    }

    drop(req_tx);
    let _ = engine_handle.join();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_cancellation_mid_batch() {
    let _guard = TEST_MUTEX.lock().unwrap();
    let fixture = match common::get_fixture() {
        Some(f) => f,
        None => return,
    };

    let (_state_tx, state_rx) = watch::channel(RuntimeState {
        mode: RuntimeMode::Running,
        degraded_mode: DegradedMode::Healthy,
        active_tokens: 0,
        max_tokens: 8192,
    });
    let (metrics_tx, _) = watch::channel(hypercore_rs::runtime::governor::EngineMetrics {
        tokens_per_sec: 0.0,
        queue_depth: 0,
        stalled: false,
        source: hypercore_rs::runtime::governor::MetricSource::LlamaEngine,
        timestamp: std::time::Instant::now(),
        latency_class: hypercore_rs::runtime::governor::LatencyClass::Compute,
    });
    let (req_tx, req_rx) = mpsc::channel(10);
    let engine = LlamaEngine::new(fixture, 8192, 4, state_rx, metrics_tx, req_rx);

    let engine_handle = std::thread::spawn(move || {
        engine.run_loop().expect("Engine panicked");
    });

    let mut tasks = Vec::new();
    for i in 0..4 {
        let (resp_tx, mut resp_rx) = mpsc::channel(100);
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();

        let req = InferenceRequest {
            request_id: format!("test-cancel-{}", i),
            prompt: "Cancel me".to_string(),
            response_tx: resp_tx,
            cancel,
            session_id: i as usize,
            priority: 1,
            timeline: RequestTimeline::default(),
            max_tokens: Some(50),
            temperature: None,
        };
        req_tx.send(req).await.unwrap();

        let handle = tokio::spawn(async move {
            let mut count = 0;
            while let Some(Ok(resp)) = resp_rx.recv().await {
                if let InferenceResponse::Token(_) = resp {
                    count += 1;
                    if count == 5 && i < 2 {
                        cancel_clone.cancel();
                    }
                }
            }
            (i, count)
        });
        tasks.push(handle);
    }

    for handle in tasks {
        let (i, count) = handle.await.unwrap();
        if i < 2 {
            assert!(
                count >= 5 && count <= 7,
                "Cancelled session {} got {} tokens",
                i,
                count
            );
        } else {
            assert!(count >= 20, "Completed session {} got {} tokens", i, count);
        }
    }

    drop(req_tx);
    let _ = engine_handle.join();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "Stress test: run with STRESS_TEST=1 or --ignored"]
async fn test_long_run_drift() {
    let _guard = TEST_MUTEX.lock().unwrap();
    let fixture = match common::get_fixture() {
        Some(f) => f,
        None => return,
    };

    let (_state_tx, state_rx) = watch::channel(RuntimeState {
        mode: RuntimeMode::Running,
        degraded_mode: DegradedMode::Healthy,
        active_tokens: 0,
        max_tokens: 8192,
    });
    let (metrics_tx, _) = watch::channel(hypercore_rs::runtime::governor::EngineMetrics {
        tokens_per_sec: 0.0,
        queue_depth: 0,
        stalled: false,
        source: hypercore_rs::runtime::governor::MetricSource::LlamaEngine,
        timestamp: std::time::Instant::now(),
        latency_class: hypercore_rs::runtime::governor::LatencyClass::Compute,
    });

    let (req_tx, req_rx) = mpsc::channel(10);
    let engine = LlamaEngine::new(fixture, 8192, 4, state_rx, metrics_tx, req_rx);

    let engine_handle = std::thread::spawn(move || {
        engine.run_loop().expect("Engine panicked");
    });

    let mut _session_id = 100;
    for _iteration in 0..10 {
        let mut tasks = Vec::new();
        for i in 0..4 {
            let (resp_tx, mut resp_rx) = mpsc::channel(100);
            let req = InferenceRequest {
                request_id: format!("test-drift-{}-{}", _iteration, i),
                prompt: "Short".to_string(),
                response_tx: resp_tx,
                cancel: CancellationToken::new(),
                session_id: (100 + i) as usize,
                priority: 1,
                timeline: RequestTimeline::default(),
                max_tokens: Some(10),
                temperature: None,
            };
            req_tx.send(req).await.unwrap();
            _session_id += 1;

            tasks.push(tokio::spawn(async move {
                let mut count = 0;
                while let Some(Ok(resp)) = resp_rx.recv().await {
                    if let InferenceResponse::Token(_) = resp {
                        count += 1;
                    }
                }
                count
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }
    }

    drop(req_tx);
    let _ = engine_handle.join();
}
