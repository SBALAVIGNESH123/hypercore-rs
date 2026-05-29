use lazy_static::lazy_static;
use std::collections::HashSet;
use std::sync::Mutex;

lazy_static! {
    static ref ACTIVE_SESSIONS: Mutex<HashSet<u64>> = Mutex::new(HashSet::new());
}

pub struct SystemContract {
    pub max_kv_caches: usize,
    pub max_queue_depth: usize,
}

pub struct InvariantGuard {
    session_id: u64,
}

#[macro_export]
macro_rules! runtime_invariant {
    ($cond:expr, $($arg:tt)+) => {
        if !($cond) {
            let msg = format!($($arg)+);
            tracing::error!("RUNTIME INVARIANT VIOLATED: {}", msg);
            panic!("RUNTIME INVARIANT VIOLATED: {}", msg);
        }
    };
}

impl InvariantGuard {
    pub fn acquire_kv_cache(session_id: u64) -> Self {
        let mut sessions = match ACTIVE_SESSIONS.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        runtime_invariant!(
            sessions.insert(session_id),
            "KV Cache already active for session {}",
            session_id
        );
        Self { session_id }
    }

    pub fn assert_queue_depth(current_depth: usize, contract: &SystemContract) {
        if current_depth > contract.max_queue_depth {
            panic!(
                "SYSTEM INVARIANT VIOLATION: Queue depth exceeded limit ({} > {})",
                current_depth, contract.max_queue_depth
            );
        }
    }
}

impl Drop for InvariantGuard {
    fn drop(&mut self) {
        let mut sessions = match ACTIVE_SESSIONS.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        sessions.remove(&self.session_id);
    }
}
