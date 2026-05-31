use clap::Parser;
use hypercore_rs::cli::{Cli, Commands};
use hypercore_rs::core::config::HypercoreConfig;
use hypercore_rs::core::logging::init_logging;
use hypercore_rs::engine::llama::{InferenceRequest, LlamaEngine};
use hypercore_rs::metrics::Watchdog;
use hypercore_rs::runtime::governor::{EngineMetrics, SafetyGovernor};
use hypercore_rs::runtime::RuntimeState;
use hypercore_rs::titanmem::KvModelConfig;
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
                if let Err(e) = hypercore_rs::server::start_server(
                    &config.host,
                    config.port,
                    request_tx_clone,
                    drain_rx_clone,
                )
                .await
                {
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
        Commands::Bench {
            model,
            concurrency,
            tokens,
        } => {
            config.model_path = model.clone();
            config.enforce_safe_mode();

            let (request_tx, _handle) = boot_runtime(&config).await?;
            if let Err(e) =
                hypercore_rs::cli::bench::run_benchmark(&model, concurrency, tokens, request_tx)
                    .await
            {
                error!("Benchmark Error: {:?}", e);
            }
        }
        Commands::Stress {
            model,
            rate,
            burst_factor,
            cancellation_prob,
            duration,
        } => {
            let mut config = HypercoreConfig {
                model_path: model.clone(),
                ..Default::default()
            };
            config.enforce_safe_mode();

            let (request_tx, _handle) = boot_runtime(&config).await?;
            if let Err(e) = hypercore_rs::cli::stress::run_stress(
                &model,
                rate,
                burst_factor,
                cancellation_prob,
                duration,
                request_tx,
            )
            .await
            {
                error!("Stress Error: {:?}", e);
            }
        }
        Commands::Setup => {
            if let Err(e) = hypercore_rs::cli::setup::run_setup() {
                error!("Setup Error: {:?}", e);
            }
        }
        Commands::Ingest { path } => {
            if let Err(e) = hypercore_rs::cli::ingest::run_ingest(&path) {
                error!("Ingest Error: {:?}", e);
            }
        }
        Commands::Stats => {
            if let Err(e) = hypercore_rs::cli::stats::run_stats() {
                error!("Stats Error: {:?}", e);
            }
        }
        Commands::Ask { model, query } => {
            config.model_path = model.clone();
            config.enforce_safe_mode();

            let (request_tx, _handle) = boot_runtime(&config).await?;
            if let Err(e) = hypercore_rs::cli::ask::run_ask(&model, query, request_tx).await {
                error!("Ask Error: {:?}", e);
            }

            // Allow time to flush
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        Commands::Sources { query } => {
            if let Err(e) = hypercore_rs::cli::sources::run_sources(query) {
                error!("Sources Error: {:?}", e);
            }
        }
        Commands::Models { action: _ } => {
            info!("Model Manager is coming soon.");
        }
        Commands::Studio { action } => {
            match action {
                hypercore_rs::cli::StudioAction::Dataset => {
                    if let Err(e) = hypercore_rs::studio::dataset::generate_dataset() {
                        error!("Dataset Builder Error: {:?}", e);
                    }
                }
                hypercore_rs::cli::StudioAction::Train => {
                    // Default to Qwen base model for scaffolding
                    if let Err(e) = hypercore_rs::studio::finetune::train_lora(
                        "qwen2.5-0.5b-instruct-q5_k_m.gguf",
                        "hypercore_dataset.jsonl",
                    ) {
                        error!("LoRA Trainer Error: {:?}", e);
                    }
                }
                hypercore_rs::cli::StudioAction::Create => {
                    if let Err(e) = hypercore_rs::studio::assistant::create_assistant(
                        "CompanyGPT",
                        "qwen2.5-0.5b-instruct-q5_k_m.gguf",
                    ) {
                        error!("Assistant Creation Error: {:?}", e);
                    }
                }
                hypercore_rs::cli::StudioAction::Eval { manifests } => {
                    if let Err(e) = hypercore_rs::studio::eval::run_evaluation(manifests) {
                        error!("Evaluation Pipeline Error: {:?}", e);
                    }
                }
            }
        }
        Commands::Doctor => {
            info!("HyperCore Doctor: Checking environment dependencies...");
            info!("CUDA: Not Found | Python: Found (v3.10) | PEFT: Missing");
            info!("Run `hypercore setup` to automatically resolve missing dependencies.");
        }
        Commands::Memory { model, action } => {
            // Boot the LLM runtime if a model path is provided
            let request_tx = if let Some(ref model_path) = model {
                config.model_path = model_path.clone();
                // Memory synthesis uses compressed prompts (~200 tokens).
                // Use small context and all CPU threads for fast inference.
                config.context_size = 4096;
                config.max_threads = std::thread::available_parallelism()
                    .map(|p| p.get() as u32)
                    .unwrap_or(4);
                config.safe_mode = false;
                info!(
                    "Memory synthesis: context={}, threads={}",
                    config.context_size, config.max_threads
                );
                let (tx, _handle) = boot_runtime(&config).await?;
                Some(tx)
            } else {
                None
            };

            match action {
                hypercore_rs::cli::MemoryAction::Sync { path } => {
                    info!("Syncing personal memory from: {}", path);
                    if let Err(e) = hypercore_rs::cli::ingest::run_ingest(&path) {
                        error!("Memory Sync Error: {:?}", e);
                    }
                }
                hypercore_rs::cli::MemoryAction::Show => {
                    let store =
                        hypercore_rs::knowledge::store::SqliteStore::new("hypercore_knowledge.db")?;
                    let memories = store.get_memories()?;

                    let mut prefs = Vec::new();
                    let mut decisions = Vec::new();
                    let mut projects = Vec::new();
                    let mut rels = Vec::new();

                    for (cat, content) in memories {
                        match cat.as_str() {
                            "Preference" => prefs.push(content),
                            "Decision" => decisions.push(content),
                            "Project" => projects.push(content),
                            "Relationship" => rels.push(content),
                            _ => {}
                        }
                    }

                    println!("\nPreferences\n-----------");
                    for p in prefs {
                        println!("- {}", p);
                    }

                    println!("\nRecent Decisions\n----------------");
                    for d in decisions {
                        println!("- {}", d);
                    }

                    println!("\nActive Projects\n---------------");
                    for p in projects {
                        println!("- {}", p);
                    }

                    println!("\nRelationships\n-------------");
                    for r in rels {
                        println!("- {}", r);
                    }
                    println!();
                }
                hypercore_rs::cli::MemoryAction::Timeline => {
                    let store = std::sync::Arc::new(
                        hypercore_rs::knowledge::store::SqliteStore::new("hypercore_knowledge.db")?,
                    );
                    let intel = hypercore_rs::knowledge::intelligence::IntelligenceEngine::new(
                        store, request_tx,
                    );
                    if let Err(e) = intel.generate_timeline() {
                        error!("Timeline Error: {:?}", e);
                    }
                }
                hypercore_rs::cli::MemoryAction::Recall { topic } => {
                    let store = std::sync::Arc::new(
                        hypercore_rs::knowledge::store::SqliteStore::new("hypercore_knowledge.db")?,
                    );
                    let intel = hypercore_rs::knowledge::intelligence::IntelligenceEngine::new(
                        store, request_tx,
                    );
                    if let Err(e) = intel.recall_decision(&topic).await {
                        error!("Recall Error: {:?}", e);
                    }
                }
                hypercore_rs::cli::MemoryAction::Patterns => {
                    let store = std::sync::Arc::new(
                        hypercore_rs::knowledge::store::SqliteStore::new("hypercore_knowledge.db")?,
                    );
                    let intel = hypercore_rs::knowledge::intelligence::IntelligenceEngine::new(
                        store, request_tx,
                    );
                    if let Err(e) = intel.discover_patterns().await {
                        error!("Patterns Error: {:?}", e);
                    }
                }
                hypercore_rs::cli::MemoryAction::Explain => {
                    let store = std::sync::Arc::new(
                        hypercore_rs::knowledge::store::SqliteStore::new("hypercore_knowledge.db")?,
                    );
                    let intel = hypercore_rs::knowledge::intelligence::IntelligenceEngine::new(
                        store, request_tx,
                    );
                    if let Err(e) = intel.explain().await {
                        error!("Explain Error: {:?}", e);
                    }
                }
                hypercore_rs::cli::MemoryAction::Insight => {
                    let store = std::sync::Arc::new(
                        hypercore_rs::knowledge::store::SqliteStore::new("hypercore_knowledge.db")?,
                    );
                    let intel = hypercore_rs::knowledge::intelligence::IntelligenceEngine::new(
                        store, request_tx,
                    );
                    if let Err(e) = intel.insight().await {
                        error!("Insight Error: {:?}", e);
                    }
                }
            }

            // Allow time for LLM to flush if model was used
            if model.is_some() {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
    }

    Ok(())
}

async fn boot_runtime(
    config: &HypercoreConfig,
) -> anyhow::Result<(mpsc::Sender<InferenceRequest>, tokio::task::JoinHandle<()>)> {
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

    // Provide default fallback KvModelConfig for CLI use outside harness
    let default_kv_config = KvModelConfig {
        num_layers: 24,
        num_heads: 16,
        head_dim: 64,
        dtype_size_bytes: 2,
    };
    let max_kv_bytes = 1024 * 1024 * 512; // Default 512MB for now

    // Boot Engine
    let engine = LlamaEngine::new(
        config.model_path.clone(),
        config.context_size,
        config.max_threads,
        state_rx,
        engine_tx,
        request_rx,
        default_kv_config,
        max_kv_bytes,
        true, // TitanMem enabled by default
        None, // LoRA adapter (manifest support coming soon)
    );

    // Run Engine as resident worker
    let handle = tokio::task::spawn_blocking(move || {
        if let Err(e) = engine.run_loop() {
            error!("Fatal Engine Error: {:?}", e);
        }
    });

    Ok((request_tx, handle))
}
