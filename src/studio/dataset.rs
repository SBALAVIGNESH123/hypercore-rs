use rusqlite::Connection;
use serde_json::json;
use std::fs::File;
use std::io::Write;
use tracing::{info, warn};

pub fn generate_dataset() -> anyhow::Result<()> {
    info!("Starting HyperCore Studio Dataset Builder...");

    let db_path = "hypercore_knowledge.db";
    let conn = match Connection::open(db_path) {
        Ok(c) => c,
        Err(_) => {
            warn!("No knowledge base found. Please run `hypercore ingest` first.");
            return Ok(());
        }
    };

    // Fetch unique documents and their content
    let mut stmt = conn.prepare("SELECT file_path, content FROM chunks LIMIT 500")?;
    let rows = stmt.query_map([], |row| {
        let path: String = row.get(0)?;
        let content: String = row.get(1)?;
        Ok((path, content))
    })?;

    let mut dataset_file = File::create("hypercore_dataset.jsonl")?;
    let mut count = 0;

    info!("Generating hybrid dataset (Retrieval, Summarization, Instruction Following)...");
    for (path, content) in rows.flatten() {
        // Type 1: Retrieval Grounding
        let type1 = json!({
            "instruction": format!("Based on the provided context, extract the core information from this excerpt of {}", path),
            "input": content,
            "output": format!("This section of {} discusses: {}", path, content.chars().take(200).collect::<String>()),
            "type": "retrieval_grounding"
        });
        writeln!(dataset_file, "{}", type1)?;

        // Type 2: Summarization
        let type2 = json!({
            "instruction": format!("Summarize the following document snippet ({})", path),
            "input": content,
            "output": format!("Summary of {}: A code or text snippet that contains specific definitions or declarations.", path),
            "type": "summarization"
        });
        writeln!(dataset_file, "{}", type2)?;

        // Type 3: Instruction following
        let type3 = json!({
            "instruction": format!("How is {} structured in the provided text?", path),
            "input": "",
            "output": format!("Based on the ingested knowledge, {} contains structured definitions as shown here: {}", path, content.chars().take(100).collect::<String>()),
            "type": "instruction_following"
        });
        writeln!(dataset_file, "{}", type3)?;

        count += 3;
    }

    info!(
        "Dataset Builder complete! Generated {} high-quality training pairs at hypercore_dataset.jsonl",
        count
    );
    Ok(())
}
