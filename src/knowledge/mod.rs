#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetrievalEngine {
    Vector,
    FTS,
}

/// Routes a query to the appropriate retrieval engine using simple heuristics.
pub fn route_query(query: &str) -> RetrievalEngine {
    let lower_query = query.to_lowercase();

    // Semantic signals -> Vector (Highest Priority)
    // Starts with question words
    if lower_query.starts_with("how ")
        || lower_query.starts_with("why ")
        || lower_query.starts_with("what ")
        || lower_query.starts_with("explain ")
    {
        return RetrievalEngine::Vector;
    }

    // Code signals -> FTS
    // Contains `_` with letters (simplistic check for snake_case/ALL_CAPS)
    if query.chars().any(|c| c == '_') && query.chars().any(|c| c.is_alphabetic()) {
        return RetrievalEngine::FTS;
    }
    // Contains `::`
    if query.contains("::") {
        return RetrievalEngine::FTS;
    }
    // Contains common file extensions
    if query.contains(".rs")
        || query.contains(".h")
        || query.contains(".cpp")
        || query.contains(".c")
        || query.contains(".py")
    {
        return RetrievalEngine::FTS;
    }
    // Contains very long words with no spaces (often identifiers or paths)
    if query
        .split_whitespace()
        .any(|word| word.len() > 14 && !word.contains(|c: char| c.is_ascii_punctuation()))
    {
        return RetrievalEngine::FTS;
    }

    // Default fallback
    RetrievalEngine::Vector
}

pub trait VectorStore {
    fn insert(&self, doc_path: &str, chunks: &[Chunk]) -> anyhow::Result<()>;
    fn search(&self, query_embedding: &[f32], limit: usize) -> anyhow::Result<Vec<SearchResult>>;
    fn fts_search(&self, query: &str, limit: usize) -> anyhow::Result<Vec<SearchResult>>;
    fn search_routed(
        &self,
        query: &str,
        query_embedding: &[f32],
        limit: usize,
    ) -> anyhow::Result<(Vec<SearchResult>, RetrievalEngine)>;
    fn stats(&self) -> anyhow::Result<StoreStats>;
}

pub struct Chunk {
    pub text: String,
    pub embedding: Vec<f32>,
    pub chunk_index: usize,
    pub start_offset: usize,
    pub end_offset: usize,
    pub file_extension: String,
}
pub struct SearchResult {
    pub doc_path: String,
    pub text: String,
    pub score: f32,
    pub chunk_index: usize,
    pub start_offset: usize,
    pub end_offset: usize,
    pub file_extension: String,
}
pub struct StoreStats {
    pub documents: usize,
    pub chunks: usize,
    pub size_bytes: u64,
    pub last_ingest: String,
}

pub mod document;
pub mod embed;
pub mod intelligence;
pub mod memory_extractor;
pub mod store;
