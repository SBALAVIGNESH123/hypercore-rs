use clap::Parser;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::token::data_array::LlamaTokenDataArray;
use std::path::PathBuf;
use std::time::{Instant, Duration};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use sysinfo::System;

#[path = "../win32_monitor.rs"]
mod win32_monitor;

#[path = "../metrics.rs"]
mod metrics;

#[derive(Parser)]
struct Args {
    #[arg(short, long)]
    model: PathBuf,
    
    #[arg(short, long)]
    prompt: String,

    #[arg(long, default_value_t = false)]
    enable_mmap: bool,

    #[arg(long, default_value_t = false)]
    enable_prefetch: bool,

    #[arg(long, default_value_t = false)]
    enable_eviction: bool,

    #[arg(long, default_value_t = false)]
    enable_budget_manager: bool,

    #[arg(long)]
    ram_budget: Option<usize>,
    
    #[arg(long)]
    mode: Option<String>, // Passed through for logging only
}

fn spawn_titanmem_prefetcher(model_path: PathBuf, done_flag: Arc<AtomicBool>, enable_prefetch: bool, enable_eviction: bool) {
    std::thread::spawn(move || {
        use std::fs::File;
        use memmap2::MmapOptions;
        use windows_sys::Win32::System::Memory::{PrefetchVirtualMemory, VirtualUnlock, WIN32_MEMORY_RANGE_ENTRY};
        use windows_sys::Win32::System::Threading::GetCurrentProcess;

        let file = File::open(&model_path).unwrap();
        let mmap = unsafe { MmapOptions::new().map(&file).unwrap() };
        let ptr = mmap.as_ptr() as *const std::ffi::c_void;
        let total_size = mmap.len();

        let chunk_size = 64 * 1024 * 1024; // 64MB chunks

        while !done_flag.load(Ordering::Relaxed) {
            let mut offset = 0;
            while offset < total_size {
                if done_flag.load(Ordering::Relaxed) {
                    break;
                }

                let size = std::cmp::min(chunk_size, total_size - offset);
                
                let mut entry = WIN32_MEMORY_RANGE_ENTRY {
                    VirtualAddress: unsafe { ptr.add(offset) as *mut std::ffi::c_void },
                    NumberOfBytes: size,
                };

                unsafe {
                    let process = GetCurrentProcess();
                    
                    if enable_eviction {
                        if offset >= 512 * 1024 * 1024 {
                            VirtualUnlock(ptr.add(offset - 512 * 1024 * 1024) as *mut std::ffi::c_void, chunk_size);
                        }
                    }
                    
                    if enable_prefetch {
                        PrefetchVirtualMemory(process, 1, &mut entry, 0);
                    }
                }

                offset += size;
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    });
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    
    let budget = args.ram_budget.unwrap_or(1024);
    
    if args.enable_budget_manager {
        win32_monitor::enforce_memory_limit(budget)
            .map_err(|e| anyhow::anyhow!("Failed to enforce RAM budget: {}", e))?;
    }
    
    let done_flag = Arc::new(AtomicBool::new(false));
    let metrics_monitor = metrics::start_metrics_monitor(done_flag.clone());

    if args.enable_mmap {
        spawn_titanmem_prefetcher(args.model.clone(), done_flag.clone(), args.enable_prefetch, args.enable_eviction);
        std::thread::sleep(Duration::from_millis(250)); // Head start
    }
    
    let backend = LlamaBackend::init()?;
    let model_params = LlamaModelParams::default();
    
    let start_load = Instant::now();
    let model = LlamaModel::load_from_file(&backend, &args.model, &model_params)
        .map_err(|e| anyhow::anyhow!("Failed to load model: {}", e))?;
    let _load_time = start_load.elapsed();
    
    let ctx_params = LlamaContextParams::default();
    let mut ctx = model.new_context(&backend, ctx_params)
        .map_err(|e| anyhow::anyhow!("Failed to create context: {}", e))?;
        
    let tokens = model.str_to_token(&args.prompt, llama_cpp_2::model::AddBos::Always)
        .map_err(|e| anyhow::anyhow!("Failed to tokenize: {}", e))?;
        
    let mut batch = LlamaBatch::new(512, 1);
    let last_index = tokens.len() - 1;
    for (i, &token) in tokens.iter().enumerate() {
        let is_last = i == last_index;
        batch.add(token, i as i32, &[0], is_last)?;
    }
    
    let start_eval = Instant::now();
    ctx.decode(&mut batch).map_err(|e| anyhow::anyhow!("Failed to decode: {}", e))?;
    let first_token_time = start_eval.elapsed();
    
    let mut n_cur = batch.n_tokens();
    let mut decoded_tokens = 0;
    
    let start_gen = Instant::now();
    let tokens_to_generate = 10; // For fast benchmarking
    
    while decoded_tokens < tokens_to_generate {
        let candidates = ctx.candidates_ith(batch.n_tokens() - 1);
        let mut candidates_p = LlamaTokenDataArray::from_iter(candidates, false);
        
        let new_token_id = candidates_p.sample_token_greedy();
        if new_token_id == model.token_eos() {
            break;
        }
        
        batch.clear();
        batch.add(new_token_id, n_cur, &[0], true)?;
        n_cur += 1;
        decoded_tokens += 1;
        
        ctx.decode(&mut batch).map_err(|e| anyhow::anyhow!("Failed to decode: {}", e))?;
    }
    
    let gen_time = start_gen.elapsed();
    let tok_sec = decoded_tokens as f64 / gen_time.as_secs_f64();
    
    done_flag.store(true, Ordering::Relaxed);
    let final_metrics = metrics_monitor.join().unwrap();

    let mut sys = System::new_all();
    sys.refresh_all();
    let cpu_info = sys.cpus().first().map(|c| c.brand()).unwrap_or("Unknown CPU").to_string();
    let ram_gb = sys.total_memory() as f64 / 1_000_000_000.0;
    let disk_read_gb = final_metrics.disk_read_bytes as f64 / 1_000_000_000.0;
    
    let total_time_s = first_token_time.as_secs_f64() + gen_time.as_secs_f64();
    let avg_disk_mb_s = if total_time_s > 0.0 {
        (final_metrics.disk_read_bytes as f64 / 1_000_000.0) / total_time_s
    } else {
        0.0
    };

    let model_size_gb = std::fs::metadata(&args.model)
        .map(|m| m.len() as f64 / 1_000_000_000.0)
        .unwrap_or(0.0);

    let out = serde_json::json!({
        "cpu": cpu_info,
        "ram_gb": ram_gb,
        "model": args.model.to_string_lossy(),
        "model_size_gb": model_size_gb,
        "budget_mb": budget,
        "first_token_latency_s": first_token_time.as_secs_f64(),
        "tokens_per_sec": tok_sec,
        "peak_ram_mb": final_metrics.peak_working_set_mb,
        "page_faults": final_metrics.process_page_faults,
        "disk_read_gb": disk_read_gb,
        "avg_disk_mb_s": avg_disk_mb_s,
        "mode": args.mode.unwrap_or_else(|| "Unknown".to_string())
    });
    
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}
