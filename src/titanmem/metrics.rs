use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::time::Duration;
use sysinfo::{System, Pid};
use windows_sys::Win32::System::Threading::GetCurrentProcessId;

#[derive(Clone, Default, Debug)]
pub struct BenchmarkMetrics {
    pub process_page_faults: u32,
    pub disk_read_bytes: u64,
    pub peak_working_set_mb: f64,
    pub system_page_reads: u64, // We might skip this if we have exact disk reads
}

pub fn start_metrics_monitor(done_flag: Arc<AtomicBool>) -> std::thread::JoinHandle<BenchmarkMetrics> {
    std::thread::spawn(move || {
        let mut sys = System::new_all();
        let pid = unsafe { GetCurrentProcessId() } as usize;
        let sysinfo_pid = Pid::from(pid);
        
        let mut final_metrics = BenchmarkMetrics::default();

        while !done_flag.load(Ordering::Relaxed) {
            sys.refresh_all();
            
            if let Some(process) = sys.process(sysinfo_pid) {
                let disk_usage = process.disk_usage();
                final_metrics.disk_read_bytes = disk_usage.total_read_bytes;
            }
            
            // Also fetch from win32_monitor for exact peak WS and page faults
            if let Ok(stats) = super::win32_monitor::get_memory_stats() {
                final_metrics.process_page_faults = stats.page_fault_count;
                final_metrics.peak_working_set_mb = stats.peak_working_set as f64 / (1024.0 * 1024.0);
            }

            std::thread::sleep(Duration::from_millis(100));
        }

        // One last fetch
        sys.refresh_all();
        if let Some(process) = sys.process(sysinfo_pid) {
            let disk_usage = process.disk_usage();
            final_metrics.disk_read_bytes = disk_usage.total_read_bytes;
        }
        if let Ok(stats) = super::win32_monitor::get_memory_stats() {
            final_metrics.process_page_faults = stats.page_fault_count;
            final_metrics.peak_working_set_mb = stats.peak_working_set as f64 / (1024.0 * 1024.0);
        }

        final_metrics
    })
}
