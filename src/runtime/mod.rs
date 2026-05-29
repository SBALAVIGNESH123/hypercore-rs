pub mod governor;
pub mod invariant;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum DegradedMode {
    #[default]
    Healthy,
    MemoryPressure,
    CriticalMemoryPressure,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum RuntimeMode {
    #[default]
    Running,
    Throttled,
    Paused,
}

#[derive(Clone, Debug, Default)]
pub struct RuntimeState {
    pub mode: RuntimeMode,
    pub active_tokens: usize,
    pub max_tokens: usize,
    pub degraded_mode: DegradedMode,
}
