use thiserror::Error;

#[derive(Error, Debug, Clone)]
pub enum RuntimeFailure {
    #[error("Engine rejected request: {0}")]
    AdmissionRejected(String),

    #[error("Client disconnected or channel dropped")]
    ClientDisconnected,

    #[error("Operation timed out: {0}")]
    Timeout(String),

    #[error("System memory pressure critical")]
    MemoryPressure,

    #[error("Internal engine fault: {0}")]
    EngineFault(String),

    #[error("Model or validation fault: {0}")]
    ModelFault(String),
}
