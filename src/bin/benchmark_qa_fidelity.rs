use hypercore_rs::knowledge::store::SqliteStore;
use hypercore_rs::knowledge::{VectorStore, route_query, RetrievalEngine};
use hypercore_rs::knowledge::embed::Embedder;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::env;
use std::time::Instant;
use std::collections::HashSet;

#[derive(Serialize, Deserialize, Debug)]
struct FidelityEval {
    correctness: u8,
    grounded_in_context: u8,
    correct_file_cited: u8,
    completeness_score: u8, // 0-2
    hallucination_flag: String, // "Y" / "N"
}

#[derive(Serialize, Deserialize)]
struct QaResult {
    question: String,
    routed_to: String,
    retrieved_context_files: Vec<String>,
    generated_answer: String,
    evaluation: Option<FidelityEval>,
}

fn call_gemini(api_key: &str, prompt: &str) -> anyhow::Result<(String, Option<FidelityEval>)> {
    let client = Client::new();
    let url = format!("https://generativelanguage.googleapis.com/v1beta/models/gemini-1.5-flash:generateContent?key={}", api_key);
    
    let sys_prompt = r#"You are a senior systems programmer evaluating a retrieval system.
Given the question and context, first write an answer.
Then, on the final lines, output a JSON object exactly matching this format:
```json
{
  "correctness": 1, // 1 if answer is technically correct, 0 if wrong
  "grounded_in_context": 1, // 1 if answer comes directly from context, 0 otherwise
  "correct_file_cited": 1, // 1 if correct file is cited, 0 otherwise
  "completeness_score": 2, // 0=incomplete, 1=partial, 2=complete
  "hallucination_flag": "N" // Y if you invented details, N otherwise
}
```"#;

    let body = json!({
        "contents": [{
            "parts": [{"text": prompt}]
        }],
        "systemInstruction": {
            "parts": [{"text": sys_prompt}]
        },
        "generationConfig": {
            "temperature": 0.0
        }
    });

    let res = client.post(&url)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()?;

    let json_resp: serde_json::Value = res.json()?;
    
    if let Some(text) = json_resp["candidates"][0]["content"]["parts"][0]["text"].as_str() {
        // Extract JSON block
        let mut eval = None;
        if let Some(start) = text.rfind("```json") {
            if let Some(end) = text[start..].find("```\n").or_else(|| text[start..].find("```\r\n")).or_else(|| text[start+7..].find("```").map(|i| i + start + 7)) {
                let json_str = &text[start+7..end];
                if let Ok(parsed) = serde_json::from_str::<FidelityEval>(json_str) {
                    eval = Some(parsed);
                }
            }
        }
        // Also check if they just output the json at the end without backticks
        if eval.is_none() {
            if let Some(start) = text.rfind('{') {
                if let Some(end) = text.rfind('}') {
                    let json_str = &text[start..end+1];
                    if let Ok(parsed) = serde_json::from_str::<FidelityEval>(json_str) {
                        eval = Some(parsed);
                    }
                }
            }
        }
        
        Ok((text.to_string(), eval))
    } else {
        Ok((format!("Error parsing response: {:?}", json_resp), None))
    }
}

fn main() -> anyhow::Result<()> {
    let api_key = match env::var("GEMINI_API_KEY") {
        Ok(key) => key,
        Err(_) => {
            println!("GEMINI_API_KEY environment variable not set. Skipping LLM generation, but will run retrieval pipeline.");
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
        "Where is tensor data stored in memory?",
        // 10 new questions
        "How does llama_get_logits work?",
        "Where is ggml_mul_mat implemented?",
        "Explain how RoPE (Rotary Positional Embedding) is calculated.",
        "Where is the vocab tokenizer implemented?",
        "How is thread synchronization handled in ggml?",
        "What does ggml_gallocr do?",
        "Where is the context size defined in llama.cpp?",
        "How do metal backend shaders work?",
        "Where is grammar parsing implemented?",
        "What is the role of llama_kv_cache_view?"
    ];

    let mut results = Vec::new();

    println!("Starting Fidelity QA Evaluation Loop...");
    
    for (i, q) in questions.iter().enumerate() {
        println!("\n[{}/{}] {}", i + 1, questions.len(), q);
        let start = Instant::now();
        
        // 1. Retrieve FTS Results
        let mut fts_hits = store.fts_search(q, 10).unwrap_or_default();
        
        // 2. Retrieve Vector Results
        let emb = embedder.embed(vec![q.to_string()])?;
        let vector_hits = store.search(&emb[0], 20).unwrap_or_default();
        
        // 3. Assemble and Deduplicate
        // Rules: FTS first, deduplicate by chunk_index/doc_path (or hash), max 3 per file, 10 total limit
        use hypercore_rs::knowledge::SearchResult;
        let mut assembled_context: Vec<SearchResult> = Vec::new();
        let mut seen_chunks = HashSet::new();
        let mut file_counts = std::collections::HashMap::new();
        
        let mut add_chunk = |hit: SearchResult| {
            if assembled_context.len() >= 10 { return; }
            let key = format!("{}:{}", hit.doc_path, hit.start_offset);
            if seen_chunks.contains(&key) { return; }
            
            let count = file_counts.entry(hit.doc_path.clone()).or_insert(0);
            if *count >= 3 { return; } // Max 3 per file
            
            *count += 1;
            seen_chunks.insert(key);
            assembled_context.push(hit);
        };
        
        // Route query - if FTS, emphasize FTS, if Vector, emphasize Vector? 
        // The user said "FTS hits first then vector score". So we just combine them.
        let engine = route_query(q);
        let engine_str = match engine {
            RetrievalEngine::FTS => "FTS",
            RetrievalEngine::Vector => "Vector",
        };

        for hit in fts_hits { add_chunk(hit); }
        for hit in vector_hits { add_chunk(hit); }
        
        let elapsed = start.elapsed();
        println!("   -> Assembled {} chunks (took {}ms) [Routed to: {}]", assembled_context.len(), elapsed.as_millis(), engine_str);
        
        let mut context_files = Vec::new();
        let mut context_text = String::new();
        
        for (j, res) in assembled_context.iter().enumerate() {
            context_files.push(res.doc_path.clone());
            context_text.push_str(&format!("--- FILE: {} ---\n{}\n\n", res.doc_path, res.text));
            if j < 3 {
                println!("      [hit] {}", res.doc_path);
            }
        }
        
        let mut generated_answer = String::from("[SKIPPED - NO API KEY]");
        let mut evaluation = None;
        if !api_key.is_empty() {
            let prompt = format!("Question: {}\n\nContext:\n{}", q, context_text);
            match call_gemini(&api_key, &prompt) {
                Ok((ans, eval)) => {
                    generated_answer = ans;
                    evaluation = eval;
                    if let Some(e) = &evaluation {
                        println!("   -> Evaluated: Score={}/2, Correct={}, Grounded={}", e.completeness_score, e.correctness, e.grounded_in_context);
                    } else {
                        println!("   -> Answer generated, but failed to parse eval JSON.");
                    }
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
            evaluation,
        });
    }

    let report_path = "qa_fidelity_report.json";
    std::fs::write(report_path, serde_json::to_string_pretty(&results)?)?;
    println!("\nBenchmark complete. Saved report to {}", report_path);

    Ok(())
}
