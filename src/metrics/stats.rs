

pub struct StatsAggregator {
    pub ttft_ms: Vec<u64>,
    pub itl_ms: Vec<u64>,
    pub queue_depths: Vec<usize>,
    pub drop_latencies_ms: Vec<u64>,
    pub kv_churn_events: u64,
    pub total_requests: u64,
    pub completed_requests: u64,
    pub rejected_requests: u64,
    pub cancelled_requests: u64,
    pub duration_ms: u64,
}

impl Default for StatsAggregator {
    fn default() -> Self {
        Self::new()
    }
}

impl StatsAggregator {
    pub fn new() -> Self {
        Self {
            ttft_ms: Vec::new(),
            itl_ms: Vec::new(),
            queue_depths: Vec::new(),
            drop_latencies_ms: Vec::new(),
            kv_churn_events: 0,
            total_requests: 0,
            completed_requests: 0,
            rejected_requests: 0,
            cancelled_requests: 0,
            duration_ms: 0,
        }
    }

    pub fn record_ttft(&mut self, ms: u64) {
        self.ttft_ms.push(ms);
    }

    pub fn record_itl(&mut self, ms: u64) {
        self.itl_ms.push(ms);
    }

    pub fn record_queue_depth(&mut self, depth: usize) {
        self.queue_depths.push(depth);
    }

    pub fn record_drop(&mut self, wait_time_ms: u64) {
        self.drop_latencies_ms.push(wait_time_ms);
    }

    pub fn record_kv_churn(&mut self) {
        self.kv_churn_events += 1;
    }

    fn percentile(data: &mut [u64], p: f64) -> u64 {
        if data.is_empty() {
            return 0;
        }
        data.sort_unstable();
        let idx = ((data.len() as f64) * p).floor() as usize;
        let idx = idx.min(data.len().saturating_sub(1));
        data[idx]
    }

    fn variance(data: &[usize]) -> f64 {
        if data.is_empty() {
            return 0.0;
        }
        let mean = data.iter().sum::<usize>() as f64 / data.len() as f64;
        let variance_sum = data.iter().map(|&x| {
            let diff = x as f64 - mean;
            diff * diff
        }).sum::<f64>();
        variance_sum / data.len() as f64
    }

    pub fn print_report(&mut self) {
        let p50_ttft = Self::percentile(&mut self.ttft_ms, 0.50);
        let p95_ttft = Self::percentile(&mut self.ttft_ms, 0.95);
        let p99_ttft = Self::percentile(&mut self.ttft_ms, 0.99);

        let p50_itl = Self::percentile(&mut self.itl_ms, 0.50);
        let p95_itl = Self::percentile(&mut self.itl_ms, 0.95);
        let p99_itl = Self::percentile(&mut self.itl_ms, 0.99);

        let qsi = Self::variance(&self.queue_depths);
        let mean_drop_ms = if self.drop_latencies_ms.is_empty() {
            0
        } else {
            self.drop_latencies_ms.iter().sum::<u64>() / self.drop_latencies_ms.len() as u64
        };

        let tps = if self.duration_ms > 0 {
            (self.itl_ms.len() as f64 / (self.duration_ms as f64 / 1000.0)) as u64
        } else {
            0
        };

        println!("\n=========================================");
        println!("🚀 Phase 3C Stress Test Results (Truth Layer)");
        println!("=========================================");
        println!("Test Duration       : {:.2} s", self.duration_ms as f64 / 1000.0);
        println!("Total Submissions   : {}", self.total_requests);
        println!("Completed Requests  : {}", self.completed_requests);
        println!("Rejected/Cancelled  : {} / {}", self.rejected_requests, self.cancelled_requests);
        println!("Global Throughput   : {} TPS", tps);
        println!("-----------------------------------------");
        println!("⏱️ Latency (Time To First Token - TTFT)");
        println!("  p50 : {} ms", p50_ttft);
        println!("  p95 : {} ms", p95_ttft);
        println!("  p99 : {} ms", p99_ttft);
        println!("-----------------------------------------");
        println!("⏱️ Latency (Inter-Token Latency - ITL)");
        println!("  p50 : {} ms", p50_itl);
        println!("  p95 : {} ms", p95_itl);
        println!("  p99 : {} ms", p99_itl);
        println!("-----------------------------------------");
        println!("📊 System Stability");
        println!("  Queue Stability Index (QSI) : {:.2} (Queue depth variance)", qsi);
        println!("  Drop Efficiency (Mean)      : {} ms", mean_drop_ms);
        println!("  KV Churn Events             : {}", self.kv_churn_events);
        println!("=========================================\n");
    }
}
