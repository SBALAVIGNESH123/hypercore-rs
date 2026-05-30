use std::mem::size_of;
use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

// QUOTA_LIMITS_HARDWS_MAX_ENABLE = 0x00000004
// This flag tells Windows to enforce a HARD maximum working set.
// Unlike Job Objects, this does NOT require Administrator privileges.
const QUOTA_LIMITS_HARDWS_MAX_ENABLE: u32 = 0x00000004;
const QUOTA_LIMITS_HARDWS_MIN_ENABLE: u32 = 0x00000002;

// SetProcessWorkingSetSizeEx is in windows-sys under Win32_System_Threading
// but we need to link it manually since windows-sys may not expose the Ex variant.
unsafe extern "system" {
    fn SetProcessWorkingSetSizeEx(
        hProcess: windows_sys::Win32::Foundation::HANDLE,
        dwMinimumWorkingSetSize: usize,
        dwMaximumWorkingSetSize: usize,
        Flags: u32,
    ) -> i32;

    fn GetProcessWorkingSetSizeEx(
        hProcess: windows_sys::Win32::Foundation::HANDLE,
        lpMinimumWorkingSetSize: *mut usize,
        lpMaximumWorkingSetSize: *mut usize,
        Flags: *mut u32,
    ) -> i32;
}

pub struct MemoryStats {
    pub peak_working_set: usize,
    pub page_fault_count: u32,
    pub current_working_set: usize,
}

pub fn enforce_memory_limit(limit_mb: usize) -> anyhow::Result<()> {
    unsafe {
        let process = GetCurrentProcess();
        let min_ws = 10 * 1024 * 1024; // 10 MB minimum
        let max_ws = limit_mb * 1024 * 1024;
        let flags = QUOTA_LIMITS_HARDWS_MIN_ENABLE | QUOTA_LIMITS_HARDWS_MAX_ENABLE;

        let res = SetProcessWorkingSetSizeEx(process, min_ws, max_ws, flags);

        if res == 0 {
            let err = windows_sys::Win32::Foundation::GetLastError();
            return Err(anyhow::anyhow!(
                "Failed to set working set limit (max={}MB). Win32 error: {}",
                limit_mb,
                err
            ));
        }

        // Verify the limit was actually applied
        let mut actual_min: usize = 0;
        let mut actual_max: usize = 0;
        let mut actual_flags: u32 = 0;
        let verify = GetProcessWorkingSetSizeEx(
            process,
            &mut actual_min,
            &mut actual_max,
            &mut actual_flags,
        );

        if verify != 0 {
            let hard_max = (actual_flags & QUOTA_LIMITS_HARDWS_MAX_ENABLE) != 0;
            eprintln!(
                "[TitanMem] Working set limit: min={}MB max={}MB hard_max={}",
                actual_min / (1024 * 1024),
                actual_max / (1024 * 1024),
                hard_max
            );
            if !hard_max {
                eprintln!("[TitanMem] WARNING: Hard working set limit was NOT enforced. Results may be unreliable.");
            }
        }
    }
    Ok(())
}

pub fn get_memory_stats() -> anyhow::Result<MemoryStats> {
    unsafe {
        let process = GetCurrentProcess();
        let mut counters: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        let cb = size_of::<PROCESS_MEMORY_COUNTERS>() as u32;

        let res = GetProcessMemoryInfo(process, &mut counters, cb);
        if res == 0 {
            return Err(anyhow::anyhow!("Failed to get process memory info"));
        }

        Ok(MemoryStats {
            peak_working_set: counters.PeakWorkingSetSize,
            page_fault_count: counters.PageFaultCount,
            current_working_set: counters.WorkingSetSize,
        })
    }
}
