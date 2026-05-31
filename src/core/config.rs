use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BackpressurePolicy {
    StallEngineIfAnyQueueFull,
    DropSlowConsumersOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HypercoreConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub model_path: String,
    pub context_size: u32,
    pub max_threads: u32,
    pub memory_limit_mb: usize,
    pub safe_mode: bool,
    pub backpressure_policy: BackpressurePolicy,
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}
fn default_port() -> u16 {
    8080
}

impl Default for HypercoreConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            model_path: "model.gguf".to_string(),
            context_size: 8192,
            max_threads: 4,
            memory_limit_mb: 6000,
            safe_mode: true,
            backpressure_policy: BackpressurePolicy::StallEngineIfAnyQueueFull,
        }
    }
}

impl HypercoreConfig {
    pub fn enforce_safe_mode(&mut self) {
        if self.safe_mode {
            self.context_size = self.context_size.min(8192);
            self.max_threads = self.max_threads.min(2);
            self.memory_limit_mb = self.memory_limit_mb.min(4096);
            self.backpressure_policy = BackpressurePolicy::StallEngineIfAnyQueueFull;
        }
    }

    pub fn load_from_file(path: &str) -> anyhow::Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        let mut config: Self = serde_yml::from_str(&contents)?;
        config.enforce_safe_mode();
        Ok(config)
    }
}
