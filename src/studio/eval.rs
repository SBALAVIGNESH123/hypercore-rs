use crate::knowledge::embed::Embedder;
use crate::knowledge::store::{SqliteStore, VectorStore};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Write;
use tracing::{info, warn};

#[derive(Serialize, Deserialize, Debug)]
pub struct EvalReport {
    pub assistant: String,
    pub total_questions: u32,
    pub answered: u32,
    pub retrieval_hits: u32,
    pub retrieval_misses: u32,
    pub retrieval_accuracy: f64,
    pub avg_top_score: f64,
    pub question_results: Vec<QuestionResult>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct QuestionResult {
    pub question: String,
    pub expected_themes: Vec<String>,
    pub top_source: String,
    pub top_score: f64,
    pub themes_found: Vec<String>,
    pub hit: bool,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BenchmarkQuestion {
    pub question: String,
    pub expected_themes: Vec<String>,
    pub source_doc: String,
}

pub fn run_evaluation(manifests: Vec<String>) -> anyhow::Result<()> {
    info!("Starting HyperCore Studio Eval Pipeline...");

    if manifests.is_empty() {
        warn!("No manifests provided for evaluation.");
        return Ok(());
    }

    // Load the benchmark questions
    let benchmark_content = std::fs::read_to_string("benchmarks/personal_ai.json")
        .unwrap_or_else(|_| "[]".to_string());
    let questions: Vec<BenchmarkQuestion> = serde_json::from_str(&benchmark_content)?;
    let total_questions = questions.len() as u32;

    if total_questions == 0 {
        warn!("No benchmark questions found in benchmarks/personal_ai.json");
        return Ok(());
    }

    info!("Loaded {} benchmark questions. Running real retrieval evaluation...", total_questions);

    // Initialize real retrieval infrastructure
    let store = SqliteStore::new("hypercore_knowledge.db")?;
    let mut embedder = Embedder::new()?;

    let mut results: Vec<QuestionResult> = Vec::new();
    let mut total_hits = 0u32;
    let mut total_score = 0.0f64;

    for (i, q) in questions.iter().enumerate() {
        info!("  [{}/{}] Evaluating: {}", i + 1, total_questions, q.question);

        // Embed the question
        let query_embedding = embedder.embed(vec![q.question.clone()])?
            .into_iter().next().unwrap();

        // Search the real knowledge store
        let search_results = store.search(&query_embedding, 3)?;

        let (top_source, top_score) = if let Some(r) = search_results.first() {
            (r.doc_path.clone(), r.score as f64)
        } else {
            ("(no results)".to_string(), 0.0)
        };

        // Check theme overlap: do any expected themes appear in the retrieved text?
        let retrieved_text: String = search_results.iter()
            .map(|r| r.text.to_lowercase())
            .collect::<Vec<_>>()
            .join(" ");

        let mut themes_found = Vec::new();
        for theme in &q.expected_themes {
            if retrieved_text.contains(&theme.to_lowercase()) {
                themes_found.push(theme.clone());
            }
        }

        let hit = !themes_found.is_empty();
        if hit { total_hits += 1; }
        total_score += top_score;

        results.push(QuestionResult {
            question: q.question.clone(),
            expected_themes: q.expected_themes.clone(),
            top_source,
            top_score,
            themes_found,
            hit,
        });
    }

    let retrieval_accuracy = if total_questions > 0 {
        total_hits as f64 / total_questions as f64
    } else { 0.0 };

    let avg_top_score = if total_questions > 0 {
        total_score / total_questions as f64
    } else { 0.0 };

    let primary_manifest = &manifests[0];

    let report = EvalReport {
        assistant: primary_manifest.clone(),
        total_questions,
        answered: total_questions,
        retrieval_hits: total_hits,
        retrieval_misses: total_questions - total_hits,
        retrieval_accuracy,
        avg_top_score,
        question_results: results,
    };

    // Print summary
    println!("\nEval Results: {}", primary_manifest);
    println!("============================================================");
    println!("  Questions:          {}", report.total_questions);
    println!("  Retrieval Hits:     {} / {}", report.retrieval_hits, report.total_questions);
    println!("  Retrieval Accuracy: {:.1}%", report.retrieval_accuracy * 100.0);
    println!("  Avg Top Score:      {:.4}", report.avg_top_score);
    println!("============================================================");

    for qr in &report.question_results {
        let status = if qr.hit { "✓" } else { "✗" };
        println!("  {} {} (score: {:.3}, themes: {:?})",
            status, qr.question, qr.top_score, qr.themes_found);
    }

    // Save JSON report
    let report_json = serde_json::to_string_pretty(&report)?;
    let report_path = format!("{}_eval_report.json", primary_manifest.replace(".yaml", ""));
    let mut file = File::create(&report_path)?;
    file.write_all(report_json.as_bytes())?;

    info!("Evaluation complete. Report saved to: {}", report_path);

    Ok(())
}
