use crate::knowledge::embed::Embedder;
use tracing::info;

pub fn run_setup() -> anyhow::Result<()> {
    info!("Starting HyperCore setup...");
    info!("Downloading local embedding models (this may take a minute on first run)...");

    // Initializing the embedder forces the download and cache.
    let mut _embedder = Embedder::new()?;
    let cache_dir = dirs::cache_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("fastembed");

    println!("\nHyperCore Knowledge environment is ready.");
    println!("---------------------------------------");
    println!("Embedding model: all-MiniLM-L6-v2");
    println!("Status: Ready");
    println!("Cache: {}", cache_dir.display());
    println!();
    Ok(())
}
