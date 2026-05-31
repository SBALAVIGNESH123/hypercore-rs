use crate::knowledge::store::SqliteStore;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::task;
use tracing::{info, warn};

/// File extensions that contain natural language worth extracting memories from.
const NATURAL_LANGUAGE_EXTENSIONS: &[&str] = &["md", "txt", "yaml", "yml", "toml", "csv", "log"];

pub struct BackgroundMemoryBuilder {
    store: Arc<SqliteStore>,
}

impl BackgroundMemoryBuilder {
    pub fn new(store: Arc<SqliteStore>) -> Self {
        Self { store }
    }

    /// Spawns the background extraction task so ingestion isn't blocked.
    pub fn start_extraction(self) {
        tokio::spawn(async move {
            let store = self.store.clone();
            let result = task::spawn_blocking(move || extract_memories_from_chunks(&store)).await;

            match result {
                Ok(Ok(count)) => info!(
                    "Background Memory Builder: Extracted {} memories from real data.",
                    count
                ),
                Ok(Err(e)) => warn!("Background Memory Builder failed: {:?}", e),
                Err(e) => warn!("Background Memory Builder panicked: {:?}", e),
            }
        });
    }
}

/// Sentence-level heuristic patterns for each memory category.
struct MemoryPattern {
    category: &'static str,
    signals: Vec<&'static str>,
}

fn get_patterns() -> Vec<MemoryPattern> {
    vec![
        MemoryPattern {
            category: "Decision",
            signals: vec![
                "decided to",
                "chose ",
                "switched to",
                "went with",
                "picked ",
                "selected ",
                "opted for",
                "migrated to",
                "replaced ",
                "moved to",
                "we chose",
                "i chose",
                "decision to",
                "agreed to use",
                "settled on",
                "committed to",
                "adopted ",
            ],
        },
        MemoryPattern {
            category: "Preference",
            signals: vec![
                "i prefer",
                "we prefer",
                "i like ",
                "prefer ",
                "better than",
                "rather than",
                "instead of",
                "strongly prefer",
                "tend to use",
                "default to",
                "consistently use",
                "always use",
            ],
        },
        MemoryPattern {
            category: "Project",
            signals: vec![
                "working on",
                "building ",
                "developing ",
                "project ",
                "started building",
                "launched ",
                "shipping ",
                "released ",
                "deployed ",
                "prototyping ",
                "implementing ",
                "designing ",
            ],
        },
        MemoryPattern {
            category: "Relationship",
            signals: vec![
                "met with",
                "collaborated with",
                "discussed with",
                "worked with",
                "partnered with",
                "team includes",
                "reporting to",
                "hired ",
                "onboarded ",
                "reviewed with",
                "pair programmed with",
            ],
        },
    ]
}

/// Reads chunks from SQLite, SKIPS code files, extracts memories from
/// natural language files only.
fn extract_memories_from_chunks(store: &SqliteStore) -> anyhow::Result<usize> {
    info!("Background Memory Builder: Reading real chunks from SQLite...");

    store.clear_memories()?;

    let all_chunks = store.get_all_chunk_texts()?;
    let total_all = all_chunks.len();

    // Filter to natural language files only
    let chunks: Vec<_> = all_chunks
        .into_iter()
        .filter(|(_path, ext, _content)| NATURAL_LANGUAGE_EXTENSIONS.contains(&ext.as_str()))
        .collect();

    let total_chunks = chunks.len();

    if total_chunks == 0 {
        info!(
            "Background Memory Builder: No natural language chunks found ({} code chunks skipped).",
            total_all
        );
        return Ok(0);
    }

    info!("Background Memory Builder: Processing {} natural language chunks ({} code chunks skipped)...",
        total_chunks, total_all - total_chunks);

    let patterns = get_patterns();
    let mut extracted: Vec<(String, String, String)> = Vec::new();
    let mut seen = HashSet::new();

    for (i, (file_path, _ext, content)) in chunks.iter().enumerate() {
        if total_chunks > 20 && (i + 1) % (total_chunks / 5).max(1) == 0 {
            let pct = ((i + 1) as f32 / total_chunks as f32 * 100.0) as u32;
            info!(
                "Building your memory graph... {}% complete ({}/{})",
                pct,
                i + 1,
                total_chunks
            );
        }

        let sentences = split_sentences(content);

        for sentence in &sentences {
            let trimmed = sentence.trim();
            if trimmed.len() < 20 || trimmed.len() > 400 {
                continue;
            }

            let lower = trimmed.to_lowercase();

            // Skip lines that look like code, config, or formatting
            if is_noise(&lower) {
                continue;
            }

            for pattern in &patterns {
                for signal in &pattern.signals {
                    if lower.contains(signal) {
                        let key = format!("{}:{}", pattern.category, &lower[..lower.len().min(80)]);
                        if seen.contains(&key) {
                            continue;
                        }
                        seen.insert(key);

                        extracted.push((
                            pattern.category.to_string(),
                            trimmed.to_string(),
                            file_path.clone(),
                        ));
                        break;
                    }
                }
            }
        }
    }

    info!(
        "Background Memory Builder: Found {} candidate memories. Inserting...",
        extracted.len()
    );

    let count = extracted.len();
    for (category, content, source) in extracted {
        if let Err(e) = store.insert_memory(&category, &content, &source) {
            warn!("Failed to insert memory: {:?}", e);
        }
    }

    info!(
        "Background Memory Builder: Semantic graph extraction complete! {} real memories stored.",
        count
    );
    Ok(count)
}

fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        current.push(ch);
        if ch == '.' || ch == '!' || ch == '?' || ch == '\n' {
            let trimmed = current.trim().to_string();
            if !trimmed.is_empty() {
                sentences.push(trimmed);
            }
            current.clear();
        }
    }

    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        sentences.push(trimmed);
    }

    sentences
}

/// Filter out noise: code fragments, markdown formatting, config, URLs.
fn is_noise(line: &str) -> bool {
    // Special character density
    let noise_chars = line
        .chars()
        .filter(|c| {
            matches!(
                c,
                '{' | '}'
                    | ';'
                    | '|'
                    | '('
                    | ')'
                    | '['
                    | ']'
                    | '<'
                    | '>'
                    | '='
                    | '&'
                    | '*'
                    | '#'
                    | '`'
                    | '\\'
                    | '@'
            )
        })
        .count();
    let ratio = noise_chars as f32 / line.len().max(1) as f32;
    if ratio > 0.05 {
        return true;
    }

    // Starts with code/markdown patterns
    if line.starts_with("```")
        || line.starts_with("- `")
        || line.starts_with("| ")
        || line.starts_with("---")
        || line.starts_with("===")
        || line.starts_with("http")
        || line.starts_with("![")
    {
        return true;
    }

    // Contains code artifacts
    if line.contains("::")
        || line.contains("->")
        || line.contains("=>")
        || line.contains("()")
        || line.contains("{}")
        || line.contains("//")
        || line.contains(".rs")
        || line.contains(".py")
        || line.contains(".js")
        || line.contains("fn ")
        || line.contains("let ")
        || line.contains("pub ")
    {
        return true;
    }

    false
}
