use serde::{Deserialize, Serialize};
use crate::config::Config;

const DEFAULT_EMBEDDING_BATCH_ITEMS: usize = 16;
const DEFAULT_EMBEDDING_BATCH_CHARS: usize = 24_000;
const DEFAULT_CHUNK_SIZE: usize = 2000;
const DEFAULT_MIN_CHUNK_SIZE: usize = 300;
const DEFAULT_CHUNK_OVERLAP: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChunkingProfile {
    pub chunk_size: usize,
    pub min_chunk_size: usize,
    pub chunk_overlap: usize,
}

pub struct Provider {
    api_key: String,
    api_base: String,
    model: String,
    embedding_dim: usize,
    client: reqwest::Client,
}

#[derive(Serialize)]
struct EmbeddingRequest {
    input: Vec<String>,
    model: String,
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

impl Provider {
    pub fn new(config: &Config) -> Self {
        Self {
            api_key: config.api_key.clone(),
            api_base: config.api_base.clone(),
            model: config.model.clone(),
            embedding_dim: config.embedding_dim,
            client: reqwest::Client::new(),
        }
    }

    pub async fn get_embeddings(&self, texts: Vec<String>) -> anyhow::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let batches = plan_embedding_batches(
            &texts,
            DEFAULT_EMBEDDING_BATCH_ITEMS,
            DEFAULT_EMBEDDING_BATCH_CHARS,
        );
        let mut all_embeddings = Vec::with_capacity(texts.len());

        for (start, end) in batches {
            let batch = texts[start..end].to_vec();
            let embeddings = self.get_embeddings_single_batch(batch).await?;
            all_embeddings.extend(embeddings);
        }

        Ok(all_embeddings)
    }

    async fn get_embeddings_single_batch(&self, texts: Vec<String>) -> anyhow::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        if self.api_base.starts_with("mock://") {
            return Ok(texts.iter().map(|text| self.mock_embedding(text)).collect());
        }

        let url = format!("{}/embeddings", self.api_base.trim_end_matches('/'));
        let res = self.client.post(url)
            .bearer_auth(&self.api_key)
            .json(&EmbeddingRequest {
                input: texts.clone(),
                model: self.model.clone(),
            })
            .send()
            .await?
            .error_for_status()?;

        let body: EmbeddingResponse = res.json().await?;

        if body.data.is_empty() {
            anyhow::bail!("API returned empty embeddings");
        }

        if body.data.len() != texts.len() {
            anyhow::bail!(
                "API returned {} embeddings for {} inputs",
                body.data.len(),
                texts.len()
            );
        }

        Ok(body.data.into_iter().map(|d| d.embedding).collect())
    }

    fn mock_embedding(&self, text: &str) -> Vec<f32> {
        let dimension = self.embedding_dim.max(1);
        let mut values = vec![0.0; dimension];
        for (index, byte) in text.bytes().enumerate() {
            values[index % dimension] += byte as f32 / 255.0;
        }
        values
    }

    pub fn clone_internal(&self) -> Self {
        Self {
            api_key: self.api_key.clone(),
            api_base: self.api_base.clone(),
            model: self.model.clone(),
            embedding_dim: self.embedding_dim,
            client: self.client.clone(),
        }
    }

    pub fn chunking_profile(&self) -> ChunkingProfile {
        let model = self.model.to_ascii_lowercase();

        if model.contains("bge-large-zh-v1.5") {
            return ChunkingProfile {
                chunk_size: 700,
                min_chunk_size: 120,
                chunk_overlap: 80,
            };
        }

        ChunkingProfile {
            chunk_size: DEFAULT_CHUNK_SIZE,
            min_chunk_size: DEFAULT_MIN_CHUNK_SIZE,
            chunk_overlap: DEFAULT_CHUNK_OVERLAP,
        }
    }
}

pub fn plan_embedding_batches(
    texts: &[String],
    max_batch_items: usize,
    max_batch_chars: usize,
) -> Vec<(usize, usize)> {
    if texts.is_empty() {
        return Vec::new();
    }

    let max_batch_items = max_batch_items.max(1);
    let max_batch_chars = max_batch_chars.max(1);

    let mut batches = Vec::new();
    let mut start = 0;
    let mut batch_chars = 0;

    for (index, text) in texts.iter().enumerate() {
        let text_chars = text.chars().count();
        let would_exceed_items = index - start >= max_batch_items;
        let would_exceed_chars = batch_chars > 0 && batch_chars + text_chars > max_batch_chars;

        if start < index && (would_exceed_items || would_exceed_chars) {
            batches.push((start, index));
            start = index;
            batch_chars = 0;
        }

        batch_chars += text_chars;

        if text_chars >= max_batch_chars {
            batches.push((start, index + 1));
            start = index + 1;
            batch_chars = 0;
        }
    }

    if start < texts.len() {
        batches.push((start, texts.len()));
    }

    batches
}
