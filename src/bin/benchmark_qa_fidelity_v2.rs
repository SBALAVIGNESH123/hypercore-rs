use hypercore_rs::knowledge::embed::Embedder;
use hypercore_rs::knowledge::store::SqliteStore;
use hypercore_rs::knowledge::{route_query, RetrievalEngine, VectorStore};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};

use std::time::Instant;

#[derive(Serialize, Deserialize, Debug, Clone)]
struct FidelityEval {
    correctness: u8,
    grounded_in_context: u8,
    correct_file_cited: u8,
    completeness_score: u8,     // 0-2
    hallucination_flag: String, // "Y" / "N"
    citation_accuracy: u8,      // 0/1
    failure_type: String, // "none", "retrieval", "context_assembly", "reasoning", "hallucination"
}

#[derive(Serialize, Deserialize, Clone)]
struct QaResult {
    question: String,
    q_type: String,
    routed_to: String,
    retrieved_context_files: Vec<String>,
    generated_answer: String,
    evaluation: Option<FidelityEval>,
}

fn call_local_llm(prompt: &str) -> anyhow::Result<(String, Option<FidelityEval>)> {
    let client = Client::new();
    let url = "http://localhost:8080/v1/chat/completions";

    let sys_prompt = r#"You are a strict systems evaluator grading a RAG pipeline.
Given the question and context, first write an answer based STRICTLY on the context.
If the context lacks the answer, say "Insufficient context."

Then, on the final lines of your response, output a JSON object EXACTLY matching this format (no markdown tags around the JSON, just the raw JSON at the very end):

{
  "correctness": 1, // 1 if answer is technically correct, 0 if wrong
  "grounded_in_context": 1, // 1 if answer comes directly from context, 0 otherwise
  "correct_file_cited": 1, // 1 if correct file is cited, 0 otherwise
  "completeness_score": 2, // 0=incomplete, 1=partial, 2=complete
  "hallucination_flag": "N", // Y if you invented details, N otherwise
  "citation_accuracy": 1, // 1 if files mentioned actually exist in context, 0 otherwise
  "failure_type": "none" // "none", "retrieval", "context_assembly", "reasoning", "hallucination"
}"#;

    let body = json!({
        "messages": [
            {"role": "system", "content": sys_prompt},
            {"role": "user", "content": prompt}
        ],
        "temperature": 0.0,
        "max_tokens": 1024
    });

    let res = client
        .post(url)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()?;

    let json_resp: serde_json::Value = res.json()?;

    if let Some(text) = json_resp["choices"][0]["message"]["content"].as_str() {
        // Robust JSON extraction: find the last '{' and last '}'
        let mut eval = None;
        if let Some(start) = text.rfind('{') {
            if let Some(end) = text.rfind('}') {
                if end > start {
                    let json_str = &text[start..=end];
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
    println!("Initializing Store and Embedder...");
    let store = SqliteStore::new("hypercore_knowledge.db")?;
    let mut embedder = Embedder::new()?;

    // Tuple of (Question, Type)
    let questions = vec![
        ("Where is KV cache allocated?", "architecture"),
        (
            "Why does llama.cpp use mmap instead of malloc?",
            "architecture",
        ),
        ("Where is attention computed?", "architecture"),
        (
            "How is quantization implemented for Q4_0?",
            "semantic_reasoning",
        ),
        ("Where is ggml_backend_buffer defined?", "identifier_lookup"),
        ("How does batching work?", "semantic_reasoning"),
        ("What is the CPU backend execution graph?", "architecture"),
        (
            "Where is llama_model_loader implemented?",
            "identifier_lookup",
        ),
        ("How are GGUF tensors loaded?", "semantic_reasoning"),
        (
            "Where is ggml_compute_forward defined?",
            "identifier_lookup",
        ),
        (
            "Where are quantization formats defined?",
            "identifier_lookup",
        ),
        (
            "How is speculative decoding implemented?",
            "semantic_reasoning",
        ),
        ("Where is llama_decode implemented?", "identifier_lookup"),
        ("How do we pass prompt tokens to the model?", "architecture"),
        ("Where is the runtime builder?", "identifier_lookup"),
        ("How are tasks scheduled?", "architecture"),
        ("How does work stealing function?", "semantic_reasoning"),
        ("How does threadpool manage threads?", "semantic_reasoning"),
        ("Why is KV cache memory unmapped?", "semantic_reasoning"),
        ("Where is tensor data stored in memory?", "architecture"),
        ("How does llama_get_logits work?", "semantic_reasoning"),
        ("Where is ggml_mul_mat implemented?", "identifier_lookup"),
        (
            "Explain how RoPE (Rotary Positional Embedding) is calculated.",
            "semantic_reasoning",
        ),
        (
            "Where is the vocab tokenizer implemented?",
            "identifier_lookup",
        ),
        (
            "How is thread synchronization handled in ggml?",
            "semantic_reasoning",
        ),
        ("What does ggml_gallocr do?", "architecture"),
        (
            "Where is the context size defined in llama.cpp?",
            "identifier_lookup",
        ),
        ("How do metal backend shaders work?", "semantic_reasoning"),
        ("Where is grammar parsing implemented?", "identifier_lookup"),
        ("What is the role of llama_kv_cache_view?", "architecture"),
    ];

    let mut results = Vec::new();
    let mut agg_stats: HashMap<String, (usize, usize)> = HashMap::new(); // Type -> (total, correct)

    println!("Starting Fidelity QA Evaluation Loop v2...");

    for (i, (q, q_type)) in questions.iter().enumerate() {
        println!("\n[{}/{}] {} [{}]", i + 1, questions.len(), q, q_type);
        let start = Instant::now();

        // 1. Retrieve FTS Results
        let fts_hits = store.fts_search(q, 10).unwrap_or_default();

        // 2. Retrieve Vector Results
        let emb = embedder.embed(vec![q.to_string()])?;
        let vector_hits = store.search(&emb[0], 20).unwrap_or_default();

        // 3. Assemble and Deduplicate
        use hypercore_rs::knowledge::SearchResult;
        let mut assembled_context: Vec<SearchResult> = Vec::new();
        let mut seen_chunks = HashSet::new();
        let mut file_counts = HashMap::new();

        let mut add_chunk = |hit: SearchResult| {
            if assembled_context.len() >= 10 {
                return;
            }
            let key = format!("{}:{}", hit.doc_path, hit.start_offset);
            if seen_chunks.contains(&key) {
                return;
            }

            let count = file_counts.entry(hit.doc_path.clone()).or_insert(0);
            if *count >= 3 {
                return;
            }

            *count += 1;
            seen_chunks.insert(key);
            assembled_context.push(hit);
        };

        let engine = route_query(q);
        let engine_str = match engine {
            RetrievalEngine::FTS => "FTS",
            RetrievalEngine::Vector => "Vector",
        };

        for hit in fts_hits {
            add_chunk(hit);
        }
        for hit in vector_hits {
            add_chunk(hit);
        }

        let elapsed = start.elapsed();
        println!(
            "   -> Assembled {} chunks (took {}ms) [Routed: {}]",
            assembled_context.len(),
            elapsed.as_millis(),
            engine_str
        );

        let mut context_files = Vec::new();
        let mut context_text = String::new();

        for res in assembled_context.iter() {
            context_files.push(res.doc_path.clone());
            context_text.push_str(&format!("--- FILE: {} ---\n{}\n\n", res.doc_path, res.text));
        }

        let prompt = format!("Question: {}\n\nContext:\n{}", q, context_text);
        let generated_answer;
        let mut evaluation = None;

        match call_local_llm(&prompt) {
            Ok((ans, eval)) => {
                generated_answer = ans;
                evaluation = eval;
                if let Some(e) = &evaluation {
                    println!("   -> Evaluated: Score={}/2, Correct={}, Grounded={}, Hallucinated={}, Failure={}", 
                        e.completeness_score, e.correctness, e.grounded_in_context, e.hallucination_flag, e.failure_type);

                    let entry = agg_stats.entry(q_type.to_string()).or_insert((0, 0));
                    entry.0 += 1;
                    if e.correctness == 1 && e.hallucination_flag == "N" {
                        entry.1 += 1;
                    }
                } else {
                    println!("   -> Answer generated, but failed to parse eval JSON.");
                }
            }
            Err(e) => {
                generated_answer = format!("API Error: {}", e);
                println!("   -> Failed to generate answer: {}", e);
            }
        }

        results.push(QaResult {
            question: q.to_string(),
            q_type: q_type.to_string(),
            routed_to: engine_str.to_string(),
            retrieved_context_files: context_files,
            generated_answer,
            evaluation,
        });
    }

    let report_path = "qa_fidelity_report_v2.json";
    std::fs::write(report_path, serde_json::to_string_pretty(&results)?)?;

    println!("\n=== AGGREGATE RESULTS ===");
    for (q_type, (total, correct)) in agg_stats.iter() {
        println!(
            "{}: {}/{} correct ({:.1}%)",
            q_type,
            correct,
            total,
            (*correct as f64 / *total as f64) * 100.0
        );
    }

    println!("\nBenchmark complete. Saved report to {}", report_path);

    Ok(())
}
