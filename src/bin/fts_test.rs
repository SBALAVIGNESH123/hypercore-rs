use rusqlite::{params, Connection};

fn main() -> anyhow::Result<()> {
    let conn = Connection::open("hypercore_knowledge.db")?;
    let count: i64 = conn
        .query_row(
            "SELECT count(*) FROM chunks_fts WHERE chunks_fts MATCH 'ggml_backend_buffer'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    println!("FTS count literal (no quotes): {}", count);

    let mut stmt = conn.prepare("SELECT count(*) FROM chunks_fts WHERE chunks_fts MATCH ?1")?;
    let count_param: i64 = stmt.query_row(params!["ggml_backend_buffer"], |row| row.get(0))?;
    println!("FTS count param (no quotes): {}", count_param);
    Ok(())
}
