use std::sync::atomic::AtomicU64;

/// Global atomic counter for unique session IDs (prevents collision panics from random IDs)
pub static SESSION_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

pub mod cli;
pub mod core;
pub mod engine;
pub mod metrics;
pub mod runtime;
pub mod server;

pub mod ui;

pub mod titanmem;
