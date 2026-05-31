use hypercore_rs::titanmem::{SessionManager, MemoryPressure, SessionMetadata, Priority};
use std::time::{Instant, Duration};
use std::thread;
use std::sync::{Arc, Mutex};
use rand::Rng;

struct SimulationResult {
    total_completed: usize,
    oom_events: usize,
    avg_latency_ms: f64,
    p95_latency_ms: f64,
    p99_latency_ms: f64,
}

fn run_simulation(use_titanmem: bool, total_ram: usize, num_sessions: usize) -> SimulationResult {
    let mut manager = SessionManager::new(total_ram);
    let mut rng = rand::thread_rng();
    
    let mut latencies = Vec::new();
    let mut oom_events = 0;
    
    let simulated_ram = Arc::new(Mutex::new(0usize));
    
    for i in 0..num_sessions {
        let kv_request_size = rng.gen_range(100_000_000..500_000_000); // 100MB to 500MB
        let metadata = SessionMetadata {
            id: i as u64,
            priority: Priority::Normal,
            context_length: 2048,
            kv_cache_size_bytes: kv_request_size,
        };
        
        let start_time = Instant::now();
        
        let mut admitted = false;
        if use_titanmem {
            admitted = manager.admit(metadata.clone());
        } else {
            // Unregulated OS baseline
            admitted = true;
        }

        if admitted {
            let mut mem = simulated_ram.lock().unwrap();
            *mem += kv_request_size;
            if *mem > total_ram {
                oom_events += 1; // Unregulated OS crashes when over RAM limit
            }
            
            // Simulate inference time
            let sleep_time = rng.gen_range(10..50);
            thread::sleep(Duration::from_millis(sleep_time));
            
            latencies.push(start_time.elapsed().as_millis() as f64);
            
            *mem -= kv_request_size;
            if use_titanmem {
                manager.release(metadata.id);
            }
        } else {
            // In TitanMem mode, it was queued. We simulate queue latency.
            let sleep_time = rng.gen_range(100..200);
            thread::sleep(Duration::from_millis(sleep_time));
            latencies.push(start_time.elapsed().as_millis() as f64);
        }
    }
    
    latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
    
    let p95_idx = (latencies.len() as f64 * 0.95) as usize;
    let p99_idx = (latencies.len() as f64 * 0.99) as usize;

    let p95 = *latencies.get(p95_idx).unwrap_or(&0.0);
    let p99 = *latencies.get(p99_idx).unwrap_or(&0.0);
    let avg = latencies.iter().sum::<f64>() / latencies.len() as f64;

    SimulationResult {
        total_completed: latencies.len(),
        oom_events,
        avg_latency_ms: avg,
        p95_latency_ms: p95,
        p99_latency_ms: p99,
    }
}

fn main() {
    println!("--- TitanMem Scientific Validation Simulator ---\n");
    
    let total_ram = 2_000_000_000; // 2 GB
    let num_sessions = 500;
    
    println!("Simulating {} concurrent sessions with 2GB Memory Constraint...\n", num_sessions);

    let baseline = run_simulation(false, total_ram, num_sessions);
    println!("[BASELINE] (No TitanMem Admission Control)");
    println!("Completed: {}", baseline.total_completed);
    println!("OOM Events (Crashes): {}", baseline.oom_events);
    println!("Avg Latency: {:.2}ms", baseline.avg_latency_ms);
    println!("p95 Latency: {:.2}ms", baseline.p95_latency_ms);
    println!("p99 Latency: {:.2}ms", baseline.p99_latency_ms);
    
    println!("\n------------------------------------------------\n");
    
    let titanmem = run_simulation(true, total_ram, num_sessions);
    println!("[TITANMEM v2] (Strict KV Cache Admission Control)");
    println!("Completed: {}", titanmem.total_completed);
    println!("OOM Events (Crashes): {}", titanmem.oom_events);
    println!("Avg Latency: {:.2}ms", titanmem.avg_latency_ms);
    println!("p95 Latency: {:.2}ms", titanmem.p95_latency_ms);
    println!("p99 Latency: {:.2}ms", titanmem.p99_latency_ms);
    
    println!("\nConclusion:");
    if titanmem.oom_events < baseline.oom_events {
        println!("TitanMem successfully prevented catastrophic OOM crashes by queuing excess KV allocations.");
    }
}
