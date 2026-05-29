pub mod bench;
pub mod chat;
pub mod stress;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "hypercore")]
#[command(about = "Stable Local LLM Runtime", long_about = None)]
pub struct Cli {
    #[arg(short, long, global = true)]
    pub config: Option<String>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Run a model in standard inference mode
    Run {
        #[arg(short, long)]
        model: String,
    },
    /// Start an interactive chat session
    Chat {
        #[arg(short, long)]
        model: String,
    },
    /// Start an API server
    Serve {
        #[arg(short, long)]
        model: String,

        #[arg(short, long, default_value_t = 8080)]
        port: u16,
    },
    /// Monitor system resources
    Monitor,
    /// Run a performance benchmark
    Bench {
        #[arg(short, long)]
        model: String,

        #[arg(short, long, default_value_t = 4)]
        concurrency: usize,

        #[arg(short, long, default_value_t = 50)]
        tokens: usize,
    },
    /// Run a stochastic stress test simulating real-world load
    Stress {
        #[arg(short, long)]
        model: String,

        /// Target request rate (requests per second) for Poisson arrivals
        #[arg(short, long, default_value_t = 10.0)]
        rate: f64,

        /// Burst multiplier to simulate sudden traffic spikes
        #[arg(short, long, default_value_t = 1.0)]
        burst_factor: f64,

        /// Probability [0,1] of a request being cancelled prematurely
        #[arg(short, long, default_value_t = 0.05)]
        cancellation_prob: f64,

        /// Test duration in seconds
        #[arg(short, long, default_value_t = 30)]
        duration: u64,
    },
    /// Manage local models
    Models {
        #[command(subcommand)]
        action: ModelAction,
    },
}

#[derive(Subcommand)]
pub enum ModelAction {
    List,
    Add { url: String },
    Remove { name: String },
}
