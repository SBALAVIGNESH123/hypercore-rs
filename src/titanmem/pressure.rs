use sysinfo::System;

pub struct MemoryPressure {
    pub system_total: usize,
    pub system_available: usize,
}

impl MemoryPressure {
    pub fn current() -> Self {
        let mut sys = System::new_all();
        sys.refresh_memory();
        Self {
            system_total: sys.total_memory() as usize,
            system_available: sys.available_memory() as usize,
        }
    }

    pub fn ratio(&self) -> f32 {
        if self.system_total == 0 {
            return 0.0;
        }
        1.0 - (self.system_available as f32 / self.system_total as f32)
    }

    pub fn is_critical(&self) -> bool {
        self.ratio() > 0.90
    }

    pub fn is_high(&self) -> bool {
        self.ratio() > 0.75
    }
}
