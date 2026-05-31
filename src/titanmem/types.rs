

pub type SessionId = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Background = 0,
    Normal = 1,
    Interactive = 2,
    System = 3,
}

#[derive(Clone, Debug)]
pub struct SessionMetadata {
    pub id: SessionId,
    pub priority: Priority,
    pub context_length: usize,
    pub kv_cache_size_bytes: usize,
}

#[derive(Clone, Debug)]
pub struct MemoryBudget {
    pub total_bytes: usize,
    pub used_bytes: usize,
}
