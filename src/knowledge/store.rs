pub use crate::knowledge::VectorStore;
use crate::knowledge::{Chunk, SearchResult, StoreStats};
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::sync::Mutex;

pub struct SqliteStore {
    conn: Mutex<Connection>,
    db_path: String,
}

impl SqliteStore {
    pub fn new(db_path: &str) -> anyhow::Result<Self> {
        let conn = Connection::open(db_path)?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS chunks (
                id INTEGER PRIMARY KEY,
                file_path TEXT NOT NULL,
                file_extension TEXT NOT NULL,
                chunk_index INTEGER NOT NULL,
                start_offset INTEGER NOT NULL,
                end_offset INTEGER NOT NULL,
                chunk_hash TEXT NOT NULL UNIQUE,
                content TEXT NOT NULL,
                embedding BLOB NOT NULL,
                ingested_at TEXT NOT NULL
            )",
            [],
        )?;
        conn.execute("DROP TABLE IF EXISTS chunks_fts", [])?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS metadata (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS memory_graph (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                category TEXT NOT NULL,
                content TEXT NOT NULL,
                source_file TEXT NOT NULL,
                timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS insight_feedback (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                insight_type TEXT NOT NULL,
                raw_insight TEXT NOT NULL,
                rating INTEGER NOT NULL,
                timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
            [],
        )?;

        conn.execute(
            "CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
                file_path, content,
                tokenize='unicode61 tokenchars ''_'''
            )",
            [],
        )?;

        conn.execute(
            "CREATE TRIGGER IF NOT EXISTS chunks_ai AFTER INSERT ON chunks BEGIN
                INSERT INTO chunks_fts(rowid, file_path, content) VALUES (new.id, new.file_path, new.content);
            END;",
            []
        )?;

        conn.execute(
            "CREATE TRIGGER IF NOT EXISTS chunks_ad AFTER DELETE ON chunks BEGIN
                INSERT INTO chunks_fts(chunks_fts, rowid, file_path, content) VALUES('delete', old.id, old.file_path, old.content);
            END;",
            []
        )?;

        conn.execute(
            "CREATE TRIGGER IF NOT EXISTS chunks_au AFTER UPDATE ON chunks BEGIN
                INSERT INTO chunks_fts(chunks_fts, rowid, file_path, content) VALUES('delete', old.id, old.file_path, old.content);
                INSERT INTO chunks_fts(rowid, file_path, content) VALUES (new.id, new.file_path, new.content);
            END;",
            []
        )?;

        // Retroactively index any existing chunks if FTS index is empty
        let count: i64 = conn
            .query_row("SELECT count(*) FROM chunks_fts", [], |row| row.get(0))
            .unwrap_or(0);
        if count == 0 {
            conn.execute(
                "INSERT INTO chunks_fts(rowid, file_path, content) SELECT id, file_path, content FROM chunks",
                []
            )?;
        }
        Ok(Self {
            conn: Mutex::new(conn),
            db_path: db_path.to_string(),
        })
    }

    fn f32_to_bytes(slice: &[f32]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(slice.len() * 4);
        for &val in slice {
            bytes.extend_from_slice(&val.to_ne_bytes());
        }
        bytes
    }

    fn bytes_to_f32(bytes: &[u8]) -> Vec<f32> {
        let mut slice = Vec::with_capacity(bytes.len() / 4);
        for chunk in bytes.chunks_exact(4) {
            slice.push(f32::from_ne_bytes(chunk.try_into().unwrap()));
        }
        slice
    }

    fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm_a == 0.0 || norm_b == 0.0 {
            0.0
        } else {
            dot / (norm_a * norm_b)
        }
    }

    pub fn insert_memory(
        &self,
        category: &str,
        content: &str,
        source_file: &str,
    ) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO memory_graph (category, content, source_file) VALUES (?1, ?2, ?3)",
            params![category, content, source_file],
        )?;
        Ok(())
    }

    pub fn get_memories(&self) -> anyhow::Result<Vec<(String, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT category, content FROM memory_graph ORDER BY id ASC")?;
        let rows = stmt.query_map([], |row| {
            let category: String = row.get(0)?;
            let content: String = row.get(1)?;
            Ok((category, content))
        })?;

        let mut memories = Vec::new();
        for row in rows {
            memories.push(row?);
        }
        Ok(memories)
    }

    pub fn get_memories_full(&self) -> anyhow::Result<Vec<(String, String, String, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT category, content, source_file, timestamp FROM memory_graph ORDER BY id ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            let category: String = row.get(0)?;
            let content: String = row.get(1)?;
            let source: String = row.get(2)?;
            let timestamp: String = row.get(3)?;
            Ok((category, content, source, timestamp))
        })?;

        let mut memories = Vec::new();
        for row in rows {
            memories.push(row?);
        }
        Ok(memories)
    }

    pub fn insert_feedback(
        &self,
        insight_type: &str,
        raw_insight: &str,
        rating: u8,
    ) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO insight_feedback (insight_type, raw_insight, rating) VALUES (?1, ?2, ?3)",
            params![insight_type, raw_insight, rating],
        )?;
        Ok(())
    }

    /// Returns all ingested chunk texts with file path and extension.
    pub fn get_all_chunk_texts(&self) -> anyhow::Result<Vec<(String, String, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT file_path, file_extension, content FROM chunks ORDER BY id ASC")?;
        let rows = stmt.query_map([], |row| {
            let file_path: String = row.get(0)?;
            let file_ext: String = row.get(1)?;
            let content: String = row.get(2)?;
            Ok((file_path, file_ext, content))
        })?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Clears all existing memories so re-extraction starts fresh.
    pub fn clear_memories(&self) -> anyhow::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM memory_graph", [])?;
        Ok(())
    }
}

impl VectorStore for SqliteStore {
    fn fts_search(&self, query: &str, limit: usize) -> anyhow::Result<Vec<SearchResult>> {
        let conn = self.conn.lock().unwrap();
        let safe_query = query
            .split_whitespace()
            .map(|w| format!("\"{}\"", w.replace("\"", "\"\"")))
            .collect::<Vec<_>>()
            .join(" ");

        let mut stmt = conn.prepare(
            "SELECT c.file_path, c.file_extension, c.chunk_index, c.start_offset, c.end_offset, c.content, f.rank
             FROM chunks_fts f
             JOIN chunks c ON f.rowid = c.id
             WHERE chunks_fts MATCH ?1
             ORDER BY f.rank
             LIMIT ?2"
        )?;

        let rows = stmt.query_map(params![safe_query, limit as i64], |row| {
            let doc_path: String = row.get(0)?;
            let file_extension: String = row.get(1)?;
            let chunk_index: usize = row.get(2)?;
            let start_offset: usize = row.get(3)?;
            let end_offset: usize = row.get(4)?;
            let text: String = row.get(5)?;
            let rank: f32 = row.get(6)?;

            // FTS5 rank is more negative for better matches.
            // We return -rank so that a higher score is better, similar to cosine similarity.
            let score = -rank;

            Ok(SearchResult {
                doc_path,
                file_extension,
                chunk_index,
                start_offset,
                end_offset,
                text,
                score,
            })
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }

        Ok(results)
    }

    fn search_routed(
        &self,
        query: &str,
        query_embedding: &[f32],
        limit: usize,
    ) -> anyhow::Result<(Vec<SearchResult>, super::RetrievalEngine)> {
        let engine = super::route_query(query);
        let results = match engine {
            super::RetrievalEngine::FTS => self.fts_search(query, limit)?,
            super::RetrievalEngine::Vector => self.search(query_embedding, limit)?,
        };
        Ok((results, engine))
    }

    fn insert(&self, doc_path: &str, chunks: &[Chunk]) -> anyhow::Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare(
            "INSERT OR IGNORE INTO chunks (
                file_path, file_extension, chunk_index, start_offset, end_offset, chunk_hash, content, embedding, ingested_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)"
        )?;

        for chunk in chunks {
            let mut hasher = Sha256::new();
            hasher.update(chunk.text.as_bytes());
            let hash_bytes = hasher.finalize();
            let chunk_hash = hash_bytes
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect::<String>();

            let blob = Self::f32_to_bytes(&chunk.embedding);
            stmt.execute(params![
                doc_path,
                chunk.file_extension,
                chunk.chunk_index,
                chunk.start_offset,
                chunk.end_offset,
                chunk_hash,
                chunk.text,
                blob,
                now
            ])?;
        }
        Ok(())
    }

    fn search(&self, query_embedding: &[f32], limit: usize) -> anyhow::Result<Vec<SearchResult>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT file_path, file_extension, chunk_index, start_offset, end_offset, content, embedding FROM chunks"
        )?;

        let rows = stmt.query_map([], |row| {
            let path: String = row.get(0)?;
            let file_extension: String = row.get(1)?;
            let chunk_index: usize = row.get(2)?;
            let start_offset: usize = row.get(3)?;
            let end_offset: usize = row.get(4)?;
            let text: String = row.get(5)?;
            let blob: Vec<u8> = row.get(6)?;
            Ok((
                path,
                file_extension,
                chunk_index,
                start_offset,
                end_offset,
                text,
                blob,
            ))
        })?;

        let mut results = Vec::new();
        for row in rows {
            let (doc_path, file_extension, chunk_index, start_offset, end_offset, text, blob) =
                row?;
            let embedding = Self::bytes_to_f32(&blob);
            let score = Self::cosine_similarity(query_embedding, &embedding);
            // Apply a minimum score threshold to drop irrelevant chunks
            if score >= 0.0 {
                results.push(SearchResult {
                    doc_path,
                    text,
                    score,
                    chunk_index,
                    start_offset,
                    end_offset,
                    file_extension,
                });
            }
        }

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(limit);
        Ok(results)
    }

    fn stats(&self) -> anyhow::Result<StoreStats> {
        let conn = self.conn.lock().unwrap();
        let documents: usize =
            conn.query_row("SELECT COUNT(DISTINCT file_path) FROM chunks", [], |row| {
                row.get(0)
            })?;
        let chunks: usize = conn.query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))?;
        let last_ingest: String = conn
            .query_row("SELECT MAX(ingested_at) FROM chunks", [], |row| row.get(0))
            .unwrap_or_else(|_| "".to_string());

        let size_bytes = if Path::new(&self.db_path).exists() {
            fs::metadata(&self.db_path)?.len()
        } else {
            0
        };

        Ok(StoreStats {
            documents,
            chunks,
            size_bytes,
            last_ingest,
        })
    }
}
