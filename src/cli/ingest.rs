use crate::knowledge::document::{chunk_text, is_supported};
use crate::knowledge::embed::Embedder;
use crate::knowledge::store::{SqliteStore, VectorStore};
use crate::knowledge::Chunk;
use std::time::Instant;
use tracing::info;
use walkdir::WalkDir;
use std::fs;

const EMBED_BATCH_SIZE: usize = 64;

pub fn run_ingest(path: &str) -> anyhow::Result<()> {
    info!("Starting streaming ingestion of {}", path);
    let start = Instant::now();

    let mut embedder = Embedder::new()?;
    let store = SqliteStore::new("hypercore_knowledge.db")?;

    let mut total_chunks = 0;
    let total_files = 0;
    let mut total_sqlite_time_secs = 0.0;

    for entry in WalkDir::new(path).into_iter().filter_entry(|e| {
        let name = e.file_name().to_string_lossy();
        name != "target" && name != ".git" && name != "node_modules" && name != "build"
    }).filter_map(|e| e.ok()) {
        if let Some(ext) = is_supported(&entry) {
            let doc_path = entry.path().to_string_lossy().to_string();
            if let Ok(content) = fs::read_to_string(entry.path()) {
                let _total_files = total_files;
                let raw_chunks = chunk_text(&content, &doc_path, 2000, 200, &ext);
                let total_file_chunks = raw_chunks.len();
                if total_file_chunks > 0 {
                    println!("[ingest] {}", doc_path);
                }

                // Process in batches of 64
                for (batch_idx, chunk_batch) in raw_chunks.chunks(EMBED_BATCH_SIZE).enumerate() {
                    let text_vec: Vec<String> = chunk_batch.iter().map(|c| c.text.clone()).collect();
                    let chunk_embeddings = embedder.embed(text_vec)?;

                    let chunks: Vec<Chunk> = chunk_batch
                        .into_iter()
                        .zip(chunk_embeddings.into_iter())
                        .map(|(raw, embedding)| Chunk { 
                            text: raw.text.clone(), 
                            embedding,
                            chunk_index: raw.chunk_index,
                            start_offset: raw.start_offset,
                            end_offset: raw.end_offset,
                            file_extension: raw.file_extension.clone(),
                        })
                        .collect();

                    let processed_chunks = std::cmp::min((batch_idx + 1) * EMBED_BATCH_SIZE, total_file_chunks);
                    println!("  chunk {} / {}", processed_chunks, total_file_chunks);
                    
                    total_chunks += chunks.len();
                    
                    let sqlite_start = Instant::now();
                    store.insert(&doc_path, &chunks)?;
                    total_sqlite_time_secs += sqlite_start.elapsed().as_secs_f64();
                }
            }
        }
    }

    let db_path = "hypercore_knowledge.db";
    let metadata = std::fs::metadata(db_path)?;
    let _db_size_mb = metadata.len() as f64 / (1024.0 * 1024.0);

    let elapsed = start.elapsed().as_secs_f64();
    let chunks_per_sec = total_chunks as f64 / elapsed;
    let sqlite_write_speed = if total_sqlite_time_secs > 0.0 {
        total_chunks as f64 / total_sqlite_time_secs
    } else {
        0.0
    };

    println!("\nStreaming Ingestion Complete");
    println!("----------------------------");
    println!("Total time: {:.1}s", elapsed);
    println!("Speed: {:.1} chunks/sec", chunks_per_sec);
    // Note: peak memory omitted for brevity, streaming ensures it stays low.
    println!("Embedding batch size: {}", EMBED_BATCH_SIZE);
    println!("SQLite write speed: {:.1} chunks/sec", sqlite_write_speed);
    
    info!("Triggering Personal Memory Graph extraction in the background...");
    let store_arc = std::sync::Arc::new(store);
    let memory_builder = crate::knowledge::memory_extractor::BackgroundMemoryBuilder::new(store_arc);
    memory_builder.start_extraction();

    // Sleep briefly to let the async task output its starting message before CLI exits
    std::thread::sleep(std::time::Duration::from_millis(50));
    
    Ok(())
}
