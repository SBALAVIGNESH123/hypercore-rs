use hypercore_rs::knowledge::store::SqliteStore;
use hypercore_rs::knowledge::VectorStore;
use std::time::Instant;

struct TestCase {
    question: &'static str,
    expected_file: &'static str,
    category: &'static str, // "semantic" or "exact"
}

fn main() -> anyhow::Result<()> {
    let store = SqliteStore::new("hypercore_knowledge.db")?;

    let test_cases = vec![
        // Exact Lookup Questions
        TestCase {
            category: "exact",
            question: "Where is ggml_backend_buffer defined?",
            expected_file: "ggml-backend.h",
        },
        TestCase {
            category: "exact",
            question: "Where is llama_model_loader implemented?",
            expected_file: "llama.cpp",
        },
        TestCase {
            category: "exact",
            question: "Where is llama_kv_cache defined?",
            expected_file: "llama.cpp",
        },
        TestCase {
            category: "exact",
            question: "Where is ggml_compute_forward defined?",
            expected_file: "ggml.c",
        },
        TestCase {
            category: "exact",
            question: "Where is llama_decode implemented?",
            expected_file: "llama.cpp",
        },
        // Semantic Questions
        TestCase {
            category: "semantic",
            question: "How is KV cache memory managed?",
            expected_file: "llama.cpp",
        },
        TestCase {
            category: "semantic",
            question: "How are GGUF tensors loaded?",
            expected_file: "llama.cpp",
        },
        TestCase {
            category: "semantic",
            question: "How does batching work?",
            expected_file: "llama.cpp",
        },
        TestCase {
            category: "semantic",
            question: "How is quantization implemented for Q4_0?",
            expected_file: "ggml-quants.c",
        },
        TestCase {
            category: "semantic",
            question: "What is the CPU backend execution graph?",
            expected_file: "ggml-backend.c",
        },
    ];

    println!("Running Retrieval Benchmark (FTS5 Only)...\n");

    let mut exact_top1 = 0;
    let mut exact_top3 = 0;
    let mut exact_top5 = 0;
    let mut exact_total = 0;

    let mut semantic_top1 = 0;
    let mut semantic_top3 = 0;
    let mut semantic_top5 = 0;
    let mut semantic_total = 0;

    for tc in test_cases {
        // Strip out stop words manually or just feed the question.
        // FTS5 will do best if we extract the core terms, but let's test how well it does
        // with the raw question using FTS5 standard MATCH.
        let fts_query = tc
            .question
            .replace("?", "")
            .replace("Where is ", "")
            .replace(" defined", "")
            .replace(" implemented", "");
        let tokens: Vec<&str> = fts_query.split_whitespace().collect();
        let fts_match_query = tokens.join(" OR ");

        let start = Instant::now();
        // Since FTS MATCH requires standard text without special tokens we just pass the raw processed query
        // wait, let's just quote the most important keyword for exact queries if we can, or just pass the processed query.
        let results = match store.fts_search(&fts_match_query, 5) {
            Ok(res) => res,
            Err(e) => {
                println!("FTS Search Error on '{}': {}", fts_match_query, e);
                vec![]
            }
        };
        let elapsed = start.elapsed();

        let mut found_rank = None;
        if results.is_empty() {
            println!("   -> FTS RETURNED 0 RESULTS for '{}'", fts_match_query);
        } else {
            for (i, res) in results.iter().enumerate() {
                println!("      [debug] rank {} path: {}", i + 1, res.doc_path);
                if res.doc_path.ends_with(tc.expected_file) {
                    found_rank = Some(i + 1);
                    break;
                }
            }
        }

        if tc.category == "exact" {
            exact_total += 1;
            if let Some(rank) = found_rank {
                if rank <= 1 {
                    exact_top1 += 1;
                }
                if rank <= 3 {
                    exact_top3 += 1;
                }
                if rank <= 5 {
                    exact_top5 += 1;
                }
            }
        } else {
            semantic_total += 1;
            if let Some(rank) = found_rank {
                if rank <= 1 {
                    semantic_top1 += 1;
                }
                if rank <= 3 {
                    semantic_top3 += 1;
                }
                if rank <= 5 {
                    semantic_top5 += 1;
                }
            }
        }

        println!(
            "Q: {} ({}) [query: {}]",
            tc.question, tc.category, fts_query
        );
        match found_rank {
            Some(rank) => println!("   -> Found at rank {} ({}ms)", rank, elapsed.as_millis()),
            None => println!("   -> NOT FOUND in top 5 ({}ms)", elapsed.as_millis()),
        }
    }

    println!("\n=== RESULTS ===");
    println!("Exact Lookup ({}):", exact_total);
    println!(
        "  Top-1: {:.1}%",
        (exact_top1 as f64 / exact_total as f64) * 100.0
    );
    println!(
        "  Top-3: {:.1}%",
        (exact_top3 as f64 / exact_total as f64) * 100.0
    );
    println!(
        "  Top-5: {:.1}%",
        (exact_top5 as f64 / exact_total as f64) * 100.0
    );

    println!("\nSemantic Lookup ({}):", semantic_total);
    println!(
        "  Top-1: {:.1}%",
        (semantic_top1 as f64 / semantic_total as f64) * 100.0
    );
    println!(
        "  Top-3: {:.1}%",
        (semantic_top3 as f64 / semantic_total as f64) * 100.0
    );
    println!(
        "  Top-5: {:.1}%",
        (semantic_top5 as f64 / semantic_total as f64) * 100.0
    );

    Ok(())
}
