use std::collections::{HashMap, HashSet};
use crate::titanmem::types::*;
use std::time::Instant;

#[derive(Clone, Debug)]
pub struct KvModelConfig {
    pub num_layers: usize,
    pub num_heads: usize,
    pub head_dim: usize,
    pub dtype_size_bytes: usize, // fp16 = 2
}

impl KvModelConfig {
    pub fn bytes_per_token(&self) -> usize {
        2 * self.num_layers * self.num_heads * self.head_dim * self.dtype_size_bytes
    }
}

pub enum KvPhase {
    Prefill,
    Decode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitanMemMode {
    Off,
    Light,
    Protective,
}

pub struct ScheduleDecision {
    pub allowed: HashSet<u64>,
    pub evict: Vec<u64>,
    pub throttle: Vec<u64>,
}

#[derive(Debug)]
pub struct SchedulerSnapshot {
    pub active_sessions: usize,
    pub kv_used_bytes: usize,
    pub queue_depth: usize,
    pub timestamp: Instant,
    pub kv_utilization_ratio: f64,
    pub avg_session_size: f64,
}

pub struct KvTrace {
    pub session_id: u64,
    pub tokens: usize,
    pub kv_bytes: usize,
    pub timestamp: Instant,
}

pub trait KvCacheObserver {
    fn on_kv_allocate(&mut self, session_id: u64, tokens: usize);
    fn on_kv_extend(&mut self, session_id: u64, batch_tokens: usize, phase: KvPhase);
    fn on_kv_evict(&mut self, session_id: u64);
    fn get_schedule(&mut self, active_sessions: &[u64]) -> ScheduleDecision;
    fn get_snapshot(&self, queue_depth: usize) -> SchedulerSnapshot;
}

pub struct KvCacheController {
    lru: Vec<SessionId>,
    usage: HashMap<SessionId, usize>,
    config: KvModelConfig,
    max_kv_bytes: usize,
    pub enabled: bool,
    last_usage_bytes: usize,
    last_snapshot_time: Instant,
    current_mode: TitanMemMode,
    protective_exit_cooldown_start: Option<Instant>,
    sps_hard_ema: f64,
    sps_soft_ema: f64,
    pub current_u_enter: f64,
    pub current_u_exit: f64,
    pub current_session_limit: usize,
    tick_evictions: usize,
    tick_violations: usize,
}

impl KvCacheController {
    pub fn new(config: KvModelConfig, max_kv_bytes: usize, enabled: bool) -> Self {
        Self {
            lru: vec![],
            usage: HashMap::new(),
            config,
            max_kv_bytes,
            enabled,
            last_usage_bytes: 0,
            last_snapshot_time: Instant::now(),
            current_mode: TitanMemMode::Light,
            protective_exit_cooldown_start: None,
            sps_hard_ema: 0.0,
            sps_soft_ema: 0.0,
            current_u_enter: 0.80,
            current_u_exit: 0.65,
            current_session_limit: 10,
            tick_evictions: 0,
            tick_violations: 0,
        }
    }

    pub fn touch(&mut self, session_id: SessionId) {
        self.lru.retain(|&id| id != session_id);
        self.lru.push(session_id);
    }

    pub fn record_usage(&mut self, session_id: SessionId, bytes: usize) {
        self.usage.insert(session_id, bytes);
    }
}

impl KvCacheObserver for KvCacheController {
    fn on_kv_allocate(&mut self, session_id: u64, tokens: usize) {
        self.touch(session_id);
        let bytes = tokens * self.config.bytes_per_token();
        self.record_usage(session_id, bytes);
    }

    fn on_kv_extend(&mut self, session_id: u64, batch_tokens: usize, phase: KvPhase) {
        self.touch(session_id);
        let multiplier = match phase {
            KvPhase::Prefill => 1.0,
            KvPhase::Decode => 1.0, // For now, linear, but could factor in caching reuse
        };
        let additional_bytes = (batch_tokens as f64 * self.config.bytes_per_token() as f64 * multiplier) as usize;
        let current = *self.usage.get(&session_id).unwrap_or(&0);
        self.record_usage(session_id, current + additional_bytes);
    }

    fn on_kv_evict(&mut self, session_id: u64) {
        self.lru.retain(|&id| id != session_id);
        self.usage.remove(&session_id);
        self.tick_evictions += 1;
    }

    fn get_schedule(&mut self, active_sessions: &[u64]) -> ScheduleDecision {
        let mut allowed = HashSet::new();
        let evict = Vec::new();
        let mut throttle = Vec::new();
        
        let current_usage = self.usage.values().sum::<usize>();
        let mut projected_usage = current_usage;

        if !self.enabled {
            for &sid in active_sessions {
                allowed.insert(sid);
            }
            return ScheduleDecision { allowed, evict, throttle };
        }

        // Adaptive Congestion Control
        let now = Instant::now();
        let dt = now.duration_since(self.last_snapshot_time).as_secs_f64().max(0.001);
        let growth_rate = if current_usage > self.last_usage_bytes {
            (current_usage - self.last_usage_bytes) as f64 / dt
        } else {
            0.0
        };
        
        self.last_usage_bytes = current_usage;
        self.last_snapshot_time = now;
        
        let utilization = current_usage as f64 / self.max_kv_bytes as f64;
        
        // Dual-Signal Adaptive Threshold Tuning
        let instantaneous_sps_hard = self.tick_violations as f64;
        let instantaneous_sps_soft = self.tick_evictions as f64;
        self.tick_violations = 0;
        self.tick_evictions = 0;
        
        self.sps_hard_ema = 0.95 * self.sps_hard_ema + 0.05 * instantaneous_sps_hard;
        self.sps_soft_ema = 0.95 * self.sps_soft_ema + 0.05 * instantaneous_sps_soft;

        let base_enter = 0.80;
        let base_exit = 0.65;
        let base_limit = 10.0;
        
        let a = 0.05; // Hard penalty tighten
        let b = 0.02; // Soft relaxation loosen
        
        let target_u_enter = (base_enter - a * self.sps_hard_ema + b * self.sps_soft_ema).clamp(0.75, 0.90);
        let target_u_exit = (base_exit - a * self.sps_hard_ema + b * self.sps_soft_ema).clamp(0.60, 0.80);
        let target_limit = (base_limit * (1.0 - 0.1 * self.sps_hard_ema)).clamp(4.0, 16.0);
        
        self.current_u_enter = 0.95 * self.current_u_enter + 0.05 * target_u_enter;
        self.current_u_exit = 0.95 * self.current_u_exit + 0.05 * target_u_exit;
        self.current_session_limit = target_limit.round() as usize;
        
        // Thresholds
        let growth_threshold = self.max_kv_bytes as f64 * 0.10; 
        
        if self.current_mode == TitanMemMode::Protective {
            // Exit Protective (Hysteresis)
            if utilization < self.current_u_exit && growth_rate < growth_threshold && active_sessions.len() < self.current_session_limit {
                if let Some(cooldown_start) = self.protective_exit_cooldown_start {
                    if now.duration_since(cooldown_start).as_secs_f64() > 3.0 {
                        self.current_mode = TitanMemMode::Light;
                        self.protective_exit_cooldown_start = None;
                    }
                } else {
                    self.protective_exit_cooldown_start = Some(now);
                }
            } else {
                self.protective_exit_cooldown_start = None;
            }
        } else {
            // Enter Protective
            if utilization > self.current_u_enter || growth_rate > growth_threshold || active_sessions.len() > self.current_session_limit {
                self.current_mode = TitanMemMode::Protective;
                self.protective_exit_cooldown_start = None;
            } else {
                self.current_mode = TitanMemMode::Light;
            }
        }

        if self.current_mode == TitanMemMode::Light {
            for &sid in active_sessions {
                allowed.insert(sid);
            }
            return ScheduleDecision { allowed, evict, throttle };
        }

        // Protective Mode
        for &sid in active_sessions {
            if projected_usage > self.max_kv_bytes {
                self.tick_violations += 1;
                if Some(sid) == self.lru.first().copied() {
                    throttle.push(sid);
                } else {
                    allowed.insert(sid);
                }
            } else {
                allowed.insert(sid);
                projected_usage += self.config.bytes_per_token(); 
            }
        }

        ScheduleDecision {
            allowed,
            evict,
            throttle,
        }
    }

    fn get_snapshot(&self, queue_depth: usize) -> SchedulerSnapshot {
        let active_sessions = self.usage.len();
        let kv_used_bytes = self.usage.values().sum();
        let avg_session_size = if active_sessions > 0 {
            kv_used_bytes as f64 / active_sessions as f64
        } else {
            0.0
        };
        
        SchedulerSnapshot {
            active_sessions,
            kv_used_bytes,
            queue_depth,
            timestamp: Instant::now(),
            kv_utilization_ratio: kv_used_bytes as f64 / self.max_kv_bytes as f64,
            avg_session_size,
        }
    }
}
