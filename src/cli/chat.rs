use crate::engine::llama::{InferenceRequest, InferenceResponse};
use std::io::Write;
use tokio::io::{self, AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::info;

pub async fn run_chat(
    model_path: &str,
    request_tx: mpsc::Sender<InferenceRequest>,
) -> anyhow::Result<()> {
    info!("Starting HYPERCORE Chat Mode...");
    info!("Model: {}", model_path);
    println!("Type '/quit' to exit.\n");

    let mut reader = BufReader::new(io::stdin());
    let mut session_id = 0;

    loop {
        print!("> ");
        std::io::stdout().flush()?;

        let mut input = String::new();
        reader.read_line(&mut input).await?;

        let input = input.trim();
        if input == "/quit" {
            break;
        }

        if input.is_empty() {
            continue;
        }

        print!("AI: ");
        std::io::stdout().flush()?;

        let (response_tx, mut response_rx) = mpsc::channel(100);
        let cancel = CancellationToken::new();
        session_id += 1;

        let req = InferenceRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            prompt: input.to_string(),
            response_tx: response_tx,
            cancel: cancel.clone(),
            session_id: session_id,
            priority: 1,
            timeline: Default::default(),
            max_tokens: None,
            temperature: None,
        };

        if let Err(e) = request_tx.send(req).await {
            tracing::error!("Failed to send chat job to engine: {:?}", e);
            break;
        }

        // Read tokens from the engine
        while let Some(res) = response_rx.recv().await {
            match res {
                Ok(InferenceResponse::Admitted) => {
                    // Just wait
                }
                Ok(InferenceResponse::Token(token)) => {
                    print!("{}", token);
                    std::io::stdout().flush()?;
                }
                Err(e) => {
                    tracing::error!("\n[Error during generation: {:?}]", e);
                    break;
                }
            }
        }
        println!("\n");
    }

    Ok(())
}
