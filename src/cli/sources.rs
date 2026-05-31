use crate::knowledge::embed::Embedder;
use crate::knowledge::store::{SqliteStore, VectorStore};
use tracing::info;

pub fn run_sources(query: String) -> anyhow::Result<()> {
    info!("Querying sources for: \"{}\"", query);

    let mut embedder = Embedder::new()?;
    let query_embedding = embedder
        .embed(vec![query.clone()])?
        .into_iter()
        .next()
        .unwrap();

    let store = SqliteStore::new("hypercore_knowledge.db")?;
    // Get up to top 10 for sources
    let results = store.search(&query_embedding, 10)?;

    if results.is_empty() {
        println!("No relevant knowledge found.");
        return Ok(());
    }

    println!("Retrieved Sources:");
    for (i, res) in results.iter().enumerate() {
        println!(
            "{}. {} (chars {}-{}) [score={:.2}]",
            i + 1,
            res.doc_path,
            res.start_offset,
            res.end_offset,
            res.score
        );
    }

    Ok(())
}
