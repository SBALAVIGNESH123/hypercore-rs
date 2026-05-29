use hypercore_rs::engine::llama::LlamaEngine;
use hypercore_rs::runtime::governor::EngineMetrics;
use hypercore_rs::runtime::RuntimeState;
use tokio::sync::watch;

#[tokio::test]
async fn test_engine_missing_model_fails_gracefully() {
    let (_state_tx, state_rx) = watch::channel(RuntimeState::default());
    let (metrics_tx, _metrics_rx) = watch::channel(EngineMetrics::default());

    let (token_tx, _token_rx) = tokio::sync::mpsc::channel(100);
    let cancel = tokio_util::sync::CancellationToken::new();
    let dispatcher = hypercore_rs::engine::dispatcher::TokenDispatcher::new(hypercore_rs::core::config::BackpressurePolicy::StallEngineIfAnyQueueFull);
    dispatcher.subscribe(token_tx);

    let engine = LlamaEngine::new(
        "non_existent_model.gguf".to_string(),
        state_rx,
        metrics_tx,
        "Hello".to_string(),
        dispatcher,
        cancel,
    );

    let result = engine.run();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("Failed to load model"));
}

#[tokio::test]
async fn test_engine_corrupted_model_fails_gracefully() {
    let (_state_tx, state_rx) = watch::channel(RuntimeState::default());
    let (metrics_tx, _metrics_rx) = watch::channel(EngineMetrics::default());

    // Create a dummy corrupted file
    let path = "corrupted_dummy.gguf";
    std::fs::write(path, b"garbage_data_that_is_not_a_valid_gguf").unwrap();

    let (token_tx, _token_rx) = tokio::sync::mpsc::channel(100);
    let cancel = tokio_util::sync::CancellationToken::new();
    let dispatcher = hypercore_rs::engine::dispatcher::TokenDispatcher::new(hypercore_rs::core::config::BackpressurePolicy::StallEngineIfAnyQueueFull);
    dispatcher.subscribe(token_tx);

    let engine = LlamaEngine::new(
        path.to_string(),
        state_rx,
        metrics_tx,
        "Hello".to_string(),
        dispatcher,
        cancel,
    );

    let result = engine.run();
    assert!(result.is_err());
    
    // Clean up
    let _ = std::fs::remove_file(path);
}
