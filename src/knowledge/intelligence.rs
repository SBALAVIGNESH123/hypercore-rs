use crate::engine::llama::{InferenceRequest, InferenceResponse};
use crate::knowledge::store::SqliteStore;
use std::cmp::Reverse;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::info;

/// Rough estimate: 1 token ≈ 4 characters for English text
const CHARS_PER_TOKEN: usize = 4;
/// Reserve tokens for the model's response
const RESPONSE_BUDGET: usize = 400;
/// Max context we'll target (conservative, fits in 4096 model context)
const MAX_PROMPT_TOKENS: usize = 2800;

pub struct IntelligenceEngine {
    store: Arc<SqliteStore>,
    request_tx: Option<mpsc::Sender<InferenceRequest>>,
}

impl IntelligenceEngine {
    pub fn new(
        store: Arc<SqliteStore>,
        request_tx: Option<mpsc::Sender<InferenceRequest>>,
    ) -> Self {
        Self { store, request_tx }
    }

    // ─── TIMELINE ──────────────────────────────────────────────────────

    pub fn generate_timeline(&self) -> anyhow::Result<()> {
        let memories = self.store.get_memories_full()?;
        if memories.is_empty() {
            println!("No memories found. Run `hypercore ingest --path <dir>` first.");
            return Ok(());
        }

        println!("\n╔══════════════════════════════════════════╗");
        println!("║        Personal Memory Timeline          ║");
        println!("╚══════════════════════════════════════════╝");

        let mut current_date = String::new();
        for (cat, content, source, timestamp) in &memories {
            let date = timestamp.split('T').next().unwrap_or(timestamp).to_string();
            if date != current_date {
                println!("\n  📅 {}", date);
                println!("  ─────────────────────────────");
                current_date = date;
            }
            let icon = match cat.as_str() {
                "Decision" => "⚖️",
                "Preference" => "💡",
                "Project" => "🚀",
                "Relationship" => "🤝",
                _ => "📌",
            };
            println!("    {} [{}] {}", icon, cat, truncate(content, 100));
            println!("       └─ source: {}", truncate(source, 60));
        }
        println!();
        Ok(())
    }

    // ─── RECALL ────────────────────────────────────────────────────────

    pub async fn recall_decision(&self, topic: &str) -> anyhow::Result<()> {
        let memories = self.store.get_memories()?;
        if memories.is_empty() {
            println!("No memories found. Run `hypercore ingest --path <dir>` first.");
            return Ok(());
        }

        let topic_lower = topic.to_lowercase();
        let mut matches: Vec<String> = Vec::new();

        for (cat, content) in &memories {
            if cat == "Decision" && content.to_lowercase().contains(&topic_lower) {
                matches.push(content.clone());
            }
        }
        if matches.is_empty() {
            for (_cat, content) in &memories {
                if content.to_lowercase().contains(&topic_lower) {
                    matches.push(content.clone());
                }
            }
        }

        println!("\n╔══════════════════════════════════════════╗");
        println!("║          Decision Recall                 ║");
        println!("╚══════════════════════════════════════════╝");

        if matches.is_empty() {
            println!("\n  No memories found related to \"{}\".\n", topic);
            return Ok(());
        }

        println!("\n  Evidence ({} memories):\n", matches.len());
        for (i, m) in matches.iter().enumerate() {
            println!("    {}. {}", i + 1, truncate(m, 110));
        }

        if let Some(tx) = &self.request_tx {
            // Compress evidence into short bullet points
            let evidence: Vec<String> = matches.iter().map(|m| truncate(m, 80)).collect();
            let evidence_text = evidence.join("\n");

            let prompt = format!(
                "Analyze why this person made decisions about \"{}\".\n\nEvidence:\n{}\n\nIn 3-4 sentences: What pattern connects these? What motivated them? Confidence?",
                topic, evidence_text
            );

            self.synthesize(tx, &prompt).await?;
        } else {
            println!("\n  💡 Add --model <path.gguf> for LLM synthesis");
        }

        println!("\n  ──────────────────────────────────────────\n");
        let rating = Self::prompt_insight_feedback();
        let _ = self
            .store
            .insert_feedback("recall", &matches.join(" | "), rating);
        Ok(())
    }

    // ─── PATTERNS ──────────────────────────────────────────────────────

    pub async fn discover_patterns(&self) -> anyhow::Result<()> {
        let memories = self.store.get_memories()?;
        if memories.is_empty() {
            println!("No memories found. Run `hypercore ingest --path <dir>` first.");
            return Ok(());
        }

        println!("\n╔══════════════════════════════════════════╗");
        println!("║          Pattern Discovery               ║");
        println!("╚══════════════════════════════════════════╝");

        let (cat_counts, freq_sorted, sources_sorted) = self.analyze_memories(&memories)?;

        println!("\n  📊 Memory Distribution:");
        for (cat, count) in &cat_counts {
            let bar = "█".repeat(*count);
            println!("    {:15} {:3} {}", cat, count, bar);
        }

        if !freq_sorted.is_empty() {
            println!("\n  🔑 Recurring Themes:");
            for (word, count) in freq_sorted.iter().take(10) {
                println!("    {:15} {}x", word, count);
            }
        }

        println!("\n  📁 Sources:");
        for (source, count) in sources_sorted.iter().take(5) {
            println!("    {:50} {}", truncate(source, 50), count);
        }

        println!(
            "\n  Total: {} memories, {} sources",
            memories.len(),
            sources_sorted.len()
        );

        if let Some(tx) = &self.request_tx {
            // Compress: cluster by category, summarize each cluster
            let compressed = self.compress_memories(&memories);
            let themes = freq_sorted
                .iter()
                .take(5)
                .map(|(w, c)| format!("{} ({}x)", w, c))
                .collect::<Vec<_>>()
                .join(", ");

            let prompt = format!(
                "Analyze this person's memory profile.\n\nMemory summary:\n{}\n\nTop themes: {}\n\nIdentify 2 non-obvious patterns. For each: state it, cite evidence, rate confidence. Be specific.",
                compressed, themes
            );

            self.synthesize(tx, &prompt).await?;
        } else {
            println!("\n  💡 Add --model <path.gguf> for LLM pattern synthesis");
        }

        println!("\n  ──────────────────────────────────────────\n");
        let rating = Self::prompt_insight_feedback();
        let _ =
            self.store
                .insert_feedback("patterns", &format!("{} memories", memories.len()), rating);
        Ok(())
    }

    // ─── EXPLAIN ("Why Am I Like This?") ───────────────────────────────

    pub async fn explain(&self) -> anyhow::Result<()> {
        let memories = self.store.get_memories()?;
        if memories.is_empty() {
            println!("No memories found. Run `hypercore ingest --path <dir>` first.");
            return Ok(());
        }

        println!("\n╔══════════════════════════════════════════╗");
        println!("║       Why Am I Like This?                ║");
        println!("╚══════════════════════════════════════════╝");

        let (cat_counts, freq_sorted, _) = self.analyze_memories(&memories)?;
        let total = memories.len();

        println!("\n  📊 Profile: {} memories", total);
        for (cat, count) in &cat_counts {
            println!("    {} {}s", count, cat.to_lowercase());
        }

        if !freq_sorted.is_empty() {
            println!("\n  🧬 Decision DNA:");
            for (i, (word, count)) in freq_sorted.iter().take(5).enumerate() {
                let conf = if *count >= 5 {
                    "High"
                } else if *count >= 3 {
                    "Med"
                } else {
                    "Low"
                };
                println!("    {}. {} ({}x, {})", i + 1, word, count, conf);
            }
        }

        println!("\n  📋 Evidence:");
        for (cat, content) in &memories {
            println!("    [{}] {}", cat, truncate(content, 90));
        }

        if let Some(tx) = &self.request_tx {
            let compressed = self.compress_memories(&memories);

            let prompt = format!(
                "You are analyzing someone based on their memories.\n\nMemory summary:\n{}\n\nWrite a brief personal analysis (4-5 sentences):\n1. What kind of thinker are they?\n2. What pattern do they repeat without noticing?\n3. What might they be overlooking?\nBe specific. Cite evidence.",
                compressed
            );

            self.synthesize(tx, &prompt).await?;
        } else {
            println!("\n  💡 Add --model <path.gguf> for deep personal synthesis");
        }

        println!("\n  ──────────────────────────────────────────\n");
        let rating = Self::prompt_insight_feedback();
        let _ = self
            .store
            .insert_feedback("explain", &format!("{} memories", total), rating);
        Ok(())
    }

    // ─── INSIGHT (Weekly Report) ───────────────────────────────────────

    pub async fn insight(&self) -> anyhow::Result<()> {
        let memories = self.store.get_memories()?;
        if memories.is_empty() {
            println!("No memories found. Run `hypercore ingest --path <dir>` first.");
            return Ok(());
        }

        let memories_full = self.store.get_memories_full()?;

        println!("\n╔══════════════════════════════════════════╗");
        println!("║        Personal Insight Report           ║");
        println!("╚══════════════════════════════════════════╝");

        let total = memories.len();
        let mut source_counts: HashMap<String, usize> = HashMap::new();
        for (_, _, source, _) in &memories_full {
            *source_counts.entry(source.clone()).or_insert(0) += 1;
        }
        let mut sources: Vec<_> = source_counts.into_iter().collect();
        sources.sort_by_key(|b| Reverse(b.1));

        println!("\n  📈 {} memories across {} sources", total, sources.len());

        if let Some((top, count)) = sources.first() {
            println!("  🏆 Richest: {} ({} memories)", truncate(top, 40), count);
        }

        for (cat, content) in &memories {
            println!("    [{}] {}", cat, truncate(content, 80));
        }

        if let Some(tx) = &self.request_tx {
            let compressed = self.compress_memories(&memories);

            let prompt = format!(
                "Generate one weekly insight from these memories.\n\nMemories:\n{}\n\nWrite exactly:\n1. OBSERVATION: One specific non-obvious pattern (1 sentence)\n2. EVIDENCE: Which memories support it (2-3 bullet points)\n3. QUESTION: One question they should ask themselves\n4. CONFIDENCE: percentage\n\nBe concise and surprising.",
                compressed
            );

            self.synthesize(tx, &prompt).await?;
        } else {
            println!("\n  💡 Add --model <path.gguf> for AI insight generation");
        }

        println!("\n  ──────────────────────────────────────────\n");
        let rating = Self::prompt_insight_feedback();
        let _ =
            self.store
                .insert_feedback("insight", &format!("weekly, {} memories", total), rating);
        Ok(())
    }

    // ─── COMPRESSION & TOKEN BUDGETING ─────────────────────────────────

    /// Clusters memories by category and produces a compressed summary
    /// that fits within the token budget.
    fn compress_memories(&self, memories: &[(String, String)]) -> String {
        let mut clusters: HashMap<String, Vec<String>> = HashMap::new();
        for (cat, content) in memories {
            clusters
                .entry(cat.clone())
                .or_default()
                .push(truncate(content, 60));
        }

        let mut summary_parts = Vec::new();
        for (cat, items) in &clusters {
            let count = items.len();
            // Take up to 3 examples per category for compression
            let examples: Vec<&str> = items.iter().take(3).map(|s| s.as_str()).collect();
            summary_parts.push(format!(
                "{} ({} total): {}",
                cat,
                count,
                examples.join("; ")
            ));
        }

        let raw = summary_parts.join("\n");

        // Token budget check
        let est_tokens = raw.len() / CHARS_PER_TOKEN;
        let budget = MAX_PROMPT_TOKENS - RESPONSE_BUDGET - 200; // 200 for system prompt

        println!("\n  ⚡ Token Budget:");
        println!("    Memories: {}", memories.len());
        println!("    Compressed to: ~{} tokens", est_tokens);
        println!("    Budget: ~{} tokens", budget);

        if est_tokens > budget {
            println!("    ⚠️  Trimming to fit...");
            // Truncate the summary to fit
            let max_chars = budget * CHARS_PER_TOKEN;
            truncate(&raw, max_chars)
        } else {
            println!("    ✅ Fits within context");
            raw
        }
    }

    /// Sends a prompt through the LlamaEngine with proper timeout.
    async fn synthesize(
        &self,
        request_tx: &mpsc::Sender<InferenceRequest>,
        prompt: &str,
    ) -> anyhow::Result<()> {
        use std::io::Write;

        let est_tokens = prompt.len() / CHARS_PER_TOKEN;
        println!("\n  📤 Sending to LLM (~{} prompt tokens)...", est_tokens);

        let (response_tx, mut response_rx) = mpsc::channel(100);

        let req = InferenceRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            prompt: prompt.to_string(),
            response_tx,
            cancel: tokio_util::sync::CancellationToken::new(),
            session_id: 998,
            priority: 0,
            timeline: Default::default(),
            max_tokens: Some(300),
            temperature: Some(0.4),
        };

        request_tx.send(req).await?;

        print!("\n  ");
        let mut token_count = 0u32;
        while let Some(msg) = response_rx.recv().await {
            match msg {
                Ok(InferenceResponse::Token(token)) => {
                    let formatted = token.replace('\n', "\n  ");
                    print!("{}", formatted);
                    std::io::stdout().flush()?;
                    token_count += 1;
                }
                Ok(InferenceResponse::Admitted) => {
                    println!("  ✅ Model admitted request, generating...");
                    print!("  ");
                    std::io::stdout().flush()?;
                }
                Err(e) => {
                    println!("\n  [Error: {:?}]", e);
                    break;
                }
            }
        }
        println!("\n  ({} tokens generated)", token_count);

        Ok(())
    }

    // ─── ANALYSIS HELPERS ──────────────────────────────────────────────

    #[allow(clippy::type_complexity)]
    fn analyze_memories(
        &self,
        memories: &[(String, String)],
    ) -> anyhow::Result<(
        HashMap<String, usize>,
        Vec<(String, usize)>,
        Vec<(String, usize)>,
    )> {
        let mut cat_counts: HashMap<String, usize> = HashMap::new();
        for (cat, _) in memories {
            *cat_counts.entry(cat.clone()).or_insert(0) += 1;
        }

        let stop_words: Vec<&str> = vec![
            "the", "a", "an", "is", "are", "was", "were", "to", "for", "of", "in", "on", "at",
            "by", "and", "or", "but", "not", "with", "from", "that", "this", "it", "as", "we", "i",
            "you", "be", "has", "had", "have", "been", "will", "would", "can", "could", "should",
            "do", "does", "did", "than", "so", "if", "then", "also", "into", "more", "use",
            "using", "used", "all", "each", "its", "our", "their", "new", "out", "up", "no",
            "when", "what", "how", "which",
        ];

        let mut word_freq: HashMap<String, usize> = HashMap::new();
        for (_cat, content) in memories {
            for word in content.to_lowercase().split_whitespace() {
                let clean: String = word.chars().filter(|c| c.is_alphanumeric()).collect();
                if clean.len() > 3 && !stop_words.contains(&clean.as_str()) {
                    *word_freq.entry(clean).or_insert(0) += 1;
                }
            }
        }

        let mut freq_sorted: Vec<(String, usize)> =
            word_freq.into_iter().filter(|(_, c)| *c > 1).collect();
        freq_sorted.sort_by_key(|b| Reverse(b.1));

        let memories_full = self.store.get_memories_full()?;
        let mut source_counts: HashMap<String, usize> = HashMap::new();
        for (_, _, source, _) in &memories_full {
            *source_counts.entry(source.clone()).or_insert(0) += 1;
        }
        let mut sources_sorted: Vec<(String, usize)> = source_counts.into_iter().collect();
        sources_sorted.sort_by_key(|b| Reverse(b.1));

        Ok((cat_counts, freq_sorted, sources_sorted))
    }

    fn prompt_insight_feedback() -> u8 {
        use std::io::{stdin, stdout, Write};

        println!("  How valuable was this insight?");
        println!("    1) Obvious");
        println!("    2) Somewhat useful");
        println!("    3) Surprising");
        println!("    4) Changed how I think");
        print!("  > ");
        let _ = stdout().flush();

        let mut input = String::new();
        let _ = stdin().read_line(&mut input);
        let rating: u8 = input.trim().parse().unwrap_or(2);
        info!("Feedback recorded: {}", rating);
        rating
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}
