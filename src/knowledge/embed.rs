use fastembed::{TextEmbedding, InitOptions, EmbeddingModel};

pub struct Embedder {
    model: TextEmbedding,
}

impl Embedder {
    pub fn new() -> anyhow::Result<Self> {
        let model = TextEmbedding::try_new(
            InitOptions::new(EmbeddingModel::AllMiniLML6V2)
                .with_show_download_progress(true)
        )?;
        Ok(Self { model })
    }

    /// Generate embeddings for a list of strings
    pub fn embed(&mut self, texts: Vec<String>) -> anyhow::Result<Vec<Vec<f32>>> {
        let embeddings = self.model.embed(texts, None)?;
        Ok(embeddings)
    }
}
