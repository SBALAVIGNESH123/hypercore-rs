use clap::Parser;
use hypercore_rs::cli::{Cli, Commands};
use hypercore_rs::core::config::HypercoreConfig;
use hypercore_rs::core::logging::init_logging;
use hypercore_rs::engine::llama::{InferenceRequest, LlamaEngine};
use hypercore_rs::metrics::Watchdog;
use hypercore_rs::runtime::governor::{EngineMetrics, SafetyGovernor};
use hypercore_rs::runtime::RuntimeState;
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tracing::{error, info, warn};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Parse CLI arguments
    let cli = Cli::parse();
    init_logging(false); // default info level
    hypercore_rs::metrics::prometheus_sink::register_metrics();
    hypercore_rs::metrics::telemetry::init_tracer();

    let (drain_tx, drain_rx) = tokio::sync::watch::channel(false);
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::mpsc::channel(1);

    tokio::spawn(async move {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to listen for event");
        warn!("Received Ctrl+C, initiating Stage 1 Graceful Shutdown (Drain)...");
        let _ = drain_tx.send(true);
        let _ = shutdown_tx.send(()).await;
    });

    // Try loading config
    let mut config = if let Some(path) = &cli.config {
        HypercoreConfig::load_from_file(path).unwrap_or_else(|e| {
            warn!("Failed to load config from {}: {:?}", path, e);
            HypercoreConfig::default()
        })
    } else if std::path::Path::new("hypercore.yaml").exists() {
        HypercoreConfig::load_from_file("hypercore.yaml").unwrap_or_else(|e| {
            warn!("Failed to load config from hypercore.yaml: {:?}", e);
            HypercoreConfig::default()
        })
    } else {
        HypercoreConfig::default()
    };

    match cli.command {
        Commands::Run { model } => {
            config.model_path = model;
            config.enforce_safe_mode();

            let (_request_tx, handle) = boot_runtime(&config).await?;

            info!("Engine is running centrally. Run command will now wait until shutdown.");
            let _ = shutdown_rx.recv().await;
            info!("Shutting down LLM context cleanly.");
            drop(_request_tx);
            let _ = handle.await;
        }
        Commands::Chat { model } => {
            config.model_path = model.clone();
            config.enforce_safe_mode();

            let (request_tx, _handle) = boot_runtime(&config).await?;
            if let Err(e) = hypercore_rs::cli::chat::run_chat(&model, request_tx).await {
                error!("Chat Error: {:?}", e);
            }
        }
        Commands::Serve { model, port } => {
            config.model_path = model.clone();
            config.port = port;
            config.enforce_safe_mode();

            let (request_tx, handle) = boot_runtime(&config).await?;
            let request_tx_clone = request_tx.clone();
            let drain_rx_clone = drain_rx.clone();
            
            // Start server in background, keeping the handle so we can abort it
            let server_handle = tokio::spawn(async move {
                if let Err(e) = hypercore_rs::server::start_server(&config.host, config.port, request_tx_clone, drain_rx_clone).await {
                    error!("API Server Error: {:?}", e);
                }
            });

            // Wait for shutdown trigger
            let _ = shutdown_rx.recv().await;
            info!("Stage 1: Draining API... no new requests will be accepted.");
            
            // Abort the server task so its clone of request_tx is dropped.
            // This is required for the engine to see EOF on request_rx.
            server_handle.abort();
            drop(request_tx);
            
            // Stage 1/2: Wait for engine to naturally exit or timeout
            match tokio::time::timeout(Duration::from_secs(60), handle).await {
                Ok(_) => {
                    info!("Stage 1 Complete: Engine drained gracefully.");
                }
                Err(_) => {
                    warn!("Stage 2: Drain timeout (60s). Force cancelling tokens (not fully implemented yet), waiting 15s...");
                    tokio::time::sleep(Duration::from_secs(15)).await;
                    warn!("Stage 3: Hard exit.");
                    std::process::exit(1);
                }
            }
        }
        Commands::Monitor => {
            info!("Monitor mode is coming soon.");
        }
        Commands::Bench { model, concurrency, tokens } => {
            config.model_path = model.clone();
            config.enforce_safe_mode();

            let (request_tx, _handle) = boot_runtime(&config).await?;
            if let Err(e) = hypercore_rs::cli::bench::run_benchmark(&model, concurrency, tokens, request_tx).await {
                error!("Benchmark Error: {:?}", e);
            }
        }
        Commands::Stress { model, rate, burst_factor, cancellation_prob, duration } => {
            let mut config = HypercoreConfig {
                model_path: model.clone(),
                ..Default::default()
            };
            config.enforce_safe_mode();

            let (request_tx, _handle) = boot_runtime(&config).await?;
            if let Err(e) = hypercore_rs::cli::stress::run_stress(&model, rate, burst_factor, cancellation_prob, duration, request_tx).await {
                error!("Stress Error: {:?}", e);
            }
        }
        Commands::Models { action: _ } => {
            info!("Model Manager is coming soon.");
        }
    }

    Ok(())
}

async fn boot_runtime(config: &HypercoreConfig) -> anyhow::Result<(mpsc::Sender<InferenceRequest>, tokio::task::JoinHandle<()>)> {
    info!("Booting HYPERCORE v1 (Central Engine)");
    info!("Model: {}", config.model_path);

    // Auto-detect RAM for First-Run UX
    let mut real_sys = sysinfo::System::new_all();
    real_sys.refresh_memory();
    let total_ram_mb = real_sys.total_memory() / (1024 * 1024);

    if config.safe_mode {
        info!("Safe Mode: ENABLED (Strict limits enforced)");
    } else {
        info!("Safe Mode: DISABLED");
        if total_ram_mb < 8192 {
            warn!("============================================================");
            warn!(
                " WARNING: You have less than 8GB RAM ({} MB detected).",
                total_ram_mb
            );
            warn!(" Running without --safe-mode is highly likely to cause OS swap thrashing.");
            warn!(" We strongly recommend using the default safe mode config.");
            warn!("============================================================");
        }
    }

    // Setup Telemetry & Governor Channels
    let (sys_tx, sys_rx) = watch::channel(Default::default());
    let (engine_tx, engine_rx) = watch::channel(EngineMetrics::default());
    let (state_tx, state_rx) = watch::channel(RuntimeState::default());

    // Boot Watchdog
    let watchdog = Watchdog::new(sys_tx.clone(), Duration::from_millis(250));
    tokio::spawn(watchdog.start());

    // Boot Governor (now respects config limits via system metrics overlay)
    let governor = SafetyGovernor::new(sys_rx.clone(), engine_rx, state_tx);
    tokio::spawn(governor.run());

    // Wait for baseline metrics
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Monitor Thread (UI)
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(1000));
        loop {
            interval.tick().await;
            let sys = sys_rx.borrow().clone();
            info!(
                "[System Load] Memory: {:.1}% | Swap: {} MB",
                sys.memory_pressure_pct,
                sys.used_swap / (1024 * 1024)
            );
        }
    });

    // Create Inference Request Queue
    let (request_tx, request_rx) = mpsc::channel::<InferenceRequest>(100);

    // Boot Engine
    let engine = LlamaEngine::new(config.model_path.clone(), config.context_size, config.max_threads, state_rx, engine_tx, request_rx);

    // Run Engine as resident worker
    let handle = tokio::task::spawn_blocking(move || {
        if let Err(e) = engine.run_loop() {
            error!("Fatal Engine Error: {:?}", e);
        }
    });

    Ok((request_tx, handle))
}
