import sqlite3

db = sqlite3.connect('hypercore_knowledge.db')
db.execute("DROP TABLE IF EXISTS chunks_fts")
db.execute("CREATE VIRTUAL TABLE chunks_fts USING fts5(file_path, content, tokenize='unicode61 tokenchars ''_''')")
db.execute("INSERT INTO chunks_fts(rowid, file_path, content) SELECT id, file_path, content FROM chunks")
db.commit()

print("FTS index rebuilt.")
print("ggml_backend_buffer count (FTS):", db.execute("SELECT count(1) FROM chunks_fts WHERE chunks_fts MATCH 'ggml_backend_buffer'").fetchone()[0])
