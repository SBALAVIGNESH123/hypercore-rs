import sqlite3
db = sqlite3.connect('hypercore_knowledge.db')
print("ggml_backend_buffer count (LIKE):", db.execute("SELECT count(1) FROM chunks WHERE content LIKE '%ggml_backend_buffer%'").fetchone()[0])
print("ggml_backend_buffer count (FTS):", db.execute("SELECT count(1) FROM chunks_fts WHERE chunks_fts MATCH 'ggml_backend_buffer'").fetchone()[0])
