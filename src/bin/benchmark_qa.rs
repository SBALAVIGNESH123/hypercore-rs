use hypercore_rs::knowledge::store::SqliteStore;
use hypercore_rs::knowledge::{VectorStore, route_query, RetrievalEngine};
use hypercore_rs::knowledge::embed::Embedder;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::env;
use std::time::Instant;

#[derive(Serialize, Deserialize)]
struct QaResult {
    question: String,
    routed_to: String,
    retrieved_context_files: Vec<String>,
    generated_answer: String,
}

fn call_gemini(api_key: &str, prompt: &str) -> anyhow::Result<String> {
    let client = Client::new();
    let url = format!("https://generativelanguage.googleapis.com/v1beta/models/gemini-1.5-flash:generateContent?key={}", api_key);
    
    let body = json!({
        "contents": [{
            "parts": [{"text": prompt}]
        }],
        "systemInstruction": {
            "parts": [{"text": "You are a senior C++/Rust systems programmer answering questions based ONLY on the provided context. If the context does not contain the answer, say 'I cannot answer this based on the provided context'."}]
        },
        "generationConfig": {
            "temperature": 0.0
        }
    });

    let res = client.post(&url)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()?;

    let json: serde_json::Value = res.json()?;
    
    if let Some(text) = json["candidates"][0]["content"]["parts"][0]["text"].as_str() {
        Ok(text.to_string())
    } else {
        Ok(format!("Error parsing response: {:?}", json))
    }
}

fn main() -> anyhow::Result<()> {
    let api_key = match env::var("GEMINI_API_KEY") {
        Ok(key) => key,
        Err(_) => {
            println!("GEMINI_API_KEY environment variable not set. Skipping LLM generation, but will run routing benchmark.");
            "".to_string()
        }
    };

    println!("Initializing Store and Embedder...");
    let store = SqliteStore::new("hypercore_knowledge.db")?;
    let mut embedder = Embedder::new()?;

    let questions = vec![
        "Where is KV cache allocated?",
        "Why does llama.cpp use mmap instead of malloc?",
        "Where is attention computed?",
        "How is quantization implemented for Q4_0?",
        "Where is ggml_backend_buffer defined?",
        "How does batching work?",
        "What is the CPU backend execution graph?",
        "Where is llama_model_loader implemented?",
        "How are GGUF tensors loaded?",
        "Where is ggml_compute_forward defined?",
        "Where are quantization formats defined?",
        "How is speculative decoding implemented?",
        "Where is llama_decode implemented?",
        "How do we pass prompt tokens to the model?",
        "Where is the runtime builder?",
        "How are tasks scheduled?",
        "How does work stealing function?",
        "How does threadpool manage threads?",
        "Why is KV cache memory unmapped?",
        "Where is tensor data stored in memory?"
    ];

    let mut results = Vec::new();

    println!("Starting Golden Evaluation Loop...");
    
    for (i, q) in questions.iter().enumerate() {
        println!("\n[{}/{}] {}", i + 1, questions.len(), q);
        let start = Instant::now();
        
        let engine = route_query(q);
        
        let search_results = match engine {
            RetrievalEngine::FTS => store.fts_search(q, 5)?,
            RetrievalEngine::Vector => {
                let emb = embedder.embed(vec![q.to_string()])?;
                store.search(&emb[0], 5)?
            }
        };
        
        let elapsed = start.elapsed();
        let engine_str = match engine {
            RetrievalEngine::FTS => "FTS",
            RetrievalEngine::Vector => "Vector",
        };
        println!("   -> Routed to: {} (took {}ms)", engine_str, elapsed.as_millis());
        
        let mut context_files = Vec::new();
        let mut context_text = String::new();
        
        for (j, res) in search_results.iter().enumerate() {
            context_files.push(res.doc_path.clone());
            context_text.push_str(&format!("--- FILE: {} ---\n{}\n\n", res.doc_path, res.text));
            if j < 3 {
                println!("      [hit] {}", res.doc_path);
            }
        }
        
        let mut generated_answer = String::from("[SKIPPED - NO API KEY]");
        if !api_key.is_empty() {
            let prompt = format!("Question: {}\n\nContext:\n{}", q, context_text);
            match call_gemini(&api_key, &prompt) {
                Ok(ans) => {
                    generated_answer = ans;
                    println!("   -> Answer generated.");
                },
                Err(e) => {
                    generated_answer = format!("API Error: {}", e);
                    println!("   -> Failed to generate answer: {}", e);
                }
            }
        }

        results.push(QaResult {
            question: q.to_string(),
            routed_to: engine_str.to_string(),
            retrieved_context_files: context_files,
            generated_answer,
        });
    }

    let report_path = "qa_report.json";
    std::fs::write(report_path, serde_json::to_string_pretty(&results)?)?;
    println!("\nBenchmark complete. Saved report to {}", report_path);

    Ok(())
}
