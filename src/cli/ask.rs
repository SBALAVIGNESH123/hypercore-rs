use crate::engine::llama::InferenceRequest;
use crate::knowledge::embed::Embedder;
use crate::knowledge::store::{SqliteStore, VectorStore};
use tokio::sync::mpsc;
use tracing::info;

pub async fn run_ask(
    _model: &str,
    query: String,
    request_tx: mpsc::Sender<InferenceRequest>,
) -> anyhow::Result<()> {
    info!("Querying knowledge base: \"{}\"", query);

    // 1. Embed Query
    let mut embedder = Embedder::new()?;
    let query_embedding = embedder
        .embed(vec![query.clone()])?
        .into_iter()
        .next()
        .unwrap();

    // 2. Retrieve Context
    let store = SqliteStore::new("hypercore_knowledge.db")?;
    let results = store.search(&query_embedding, 3)?;

    if results.is_empty() {
        println!("No relevant knowledge found.");
        return Ok(());
    }

    println!("Answer:");
    // Stream response
    use std::io::Write;
    std::io::stdout().flush()?;

    // 3. Build Augmented Prompt
    let mut context_text = String::new();
    let mut sources_output = String::new();
    for (i, res) in results.iter().enumerate() {
        context_text.push_str(&format!("--- Document [{}] ---\n{}\n", i + 1, res.text));
        sources_output.push_str(&format!(
            "{}. {} (chars {}-{})\n",
            i + 1,
            res.doc_path,
            res.start_offset,
            res.end_offset
        ));
    }

    let augmented_prompt = format!(
        "You are an AI assistant powered by HyperCore. Answer the user's question based strictly on the provided context.\n\nContext:\n{}\n\nQuestion: {}\n",
        context_text, query
    );

    // Save sources for after the LLM completes
    let mut footer = String::new();
    footer.push_str("\n\nRetrieved Sources:\n");
    footer.push_str(&sources_output);
    footer.push_str("\nRetrieval Scores:\n");
    for res in &results {
        footer.push_str(&format!("{:.2}\n", res.score));
    }

    // 4. Run Inference (re-using chat scaffolding)
    let (response_tx, mut response_rx) = tokio::sync::mpsc::channel(100);

    let req = InferenceRequest {
        request_id: uuid::Uuid::new_v4().to_string(),
        prompt: augmented_prompt,
        response_tx,
        cancel: tokio_util::sync::CancellationToken::new(),
        session_id: 999, // Random unique enough session ID for now
        priority: 0,
        timeline: Default::default(),
        max_tokens: Some(300),
        temperature: Some(0.1), // low temp for factual QA
    };

    request_tx.send(req).await?;

    // Stream response
    print!("> ");
    std::io::stdout().flush()?;

    while let Some(msg) = response_rx.recv().await {
        match msg {
            Ok(crate::engine::llama::InferenceResponse::Token(token)) => {
                print!("{}", token);
                std::io::stdout().flush()?;
            }
            Ok(crate::engine::llama::InferenceResponse::Admitted) => {
                // Do nothing
            }
            Err(e) => {
                println!("\n[Error: {:?}]", e);
                break;
            }
        }
    }

    println!("{}", footer);

    Ok(())
}
