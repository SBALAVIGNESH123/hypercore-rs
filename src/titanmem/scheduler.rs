use std::collections::HashMap;
use crate::titanmem::types::*;

pub struct SessionManager {
    sessions: HashMap<SessionId, SessionMetadata>,
    budget: MemoryBudget,
    max_queue_size: usize,
    queue: Vec<SessionMetadata>,
}

impl SessionManager {
    pub fn new(total_memory: usize) -> Self {
        Self {
            sessions: HashMap::new(),
            budget: MemoryBudget {
                total_bytes: total_memory,
                used_bytes: 0,
            },
            max_queue_size: 100,
            queue: vec![],
        }
    }

    pub fn can_admit(&self, required_bytes: usize) -> bool {
        self.budget.used_bytes + required_bytes <= self.budget.total_bytes
    }

    pub fn admit(&mut self, session: SessionMetadata) -> bool {
        if self.can_admit(session.kv_cache_size_bytes) {
            self.budget.used_bytes += session.kv_cache_size_bytes;
            self.sessions.insert(session.id, session);
            true
        } else {
            if self.queue.len() < self.max_queue_size {
                self.queue.push(session);
            }
            false
        }
    }

    pub fn release(&mut self, session_id: SessionId) {
        if let Some(s) = self.sessions.remove(&session_id) {
            self.budget.used_bytes -= s.kv_cache_size_bytes;
        }
    }

    pub fn pressure_ratio(&self) -> f32 {
        self.budget.used_bytes as f32 / self.budget.total_bytes as f32
    }
}
