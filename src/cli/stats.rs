use crate::knowledge::store::{SqliteStore, VectorStore};

pub fn run_stats() -> anyhow::Result<()> {
    let store = SqliteStore::new("hypercore_knowledge.db")?;
    let stats = store.stats()?;

    println!("\nKnowledge Base");
    println!("--------------");
    println!("Documents: {}", stats.documents);
    println!("Chunks: {}", stats.chunks);
    println!("Embeddings: {}", stats.chunks);
    println!(
        "Database: {:.1} MB",
        stats.size_bytes as f64 / (1024.0 * 1024.0)
    );
    println!(
        "Last Updated: {}",
        stats
            .last_ingest
            .split('T')
            .next()
            .unwrap_or(&stats.last_ingest)
    );
    println!();

    Ok(())
}
