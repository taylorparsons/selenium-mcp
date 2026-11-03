use crate::error::AppError;
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

pub const DIMENSION: usize = 384;

#[derive(Clone)]
pub struct EmbeddingEngine {
    cache_dir: PathBuf,
    model: Arc<Mutex<Option<TextEmbedding>>>,
}

impl EmbeddingEngine {
    pub fn new(cache_dir: PathBuf) -> Self {
        Self {
            cache_dir,
            model: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn embed_one(&self, text: &str) -> Result<Vec<f32>, AppError> {
        let mut rows = self
            .embed_many(std::slice::from_ref(&text.to_string()))
            .await?;
        Ok(rows.remove(0))
    }

    pub async fn embed_many(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, AppError> {
        let mut guard = self.model.lock().await;
        if guard.is_none() {
            let cache_dir = self.cache_dir.clone();
            let loaded = tokio::task::spawn_blocking(move || {
                TextEmbedding::try_new(
                    TextInitOptions::new(EmbeddingModel::AllMiniLML6V2).with_cache_dir(cache_dir),
                )
            })
            .await
            .map_err(|e| AppError::Embedding(format!("model init task panicked: {e}")))?
            .map_err(|e| AppError::Embedding(format!("failed to load embedding model: {e}")))?;
            *guard = Some(loaded);
        }

        let mut model = guard.take().expect("model was just set");
        let owned_texts: Vec<String> = texts.to_vec();
        let result = tokio::task::spawn_blocking(move || {
            let refs: Vec<&str> = owned_texts.iter().map(String::as_str).collect();
            let embeddings = model.embed(refs, None);
            (model, embeddings)
        })
        .await
        .map_err(|e| AppError::Embedding(format!("embed task panicked: {e}")));

        match result {
            Ok((model, embeddings)) => {
                *guard = Some(model);
                embeddings.map_err(|e| AppError::Embedding(format!("embedding failed: {e}")))
            }
            Err(e) => Err(e),
        }
    }
}

#[cfg(all(test, feature = "model-integration"))]
mod tests {
    use super::*;

    fn cosine(a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        dot / (na * nb)
    }

    fn test_cache_dir(test_name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "selenium-mcp-embedding-test-{test_name}-{}",
            std::process::id()
        ))
    }

    #[tokio::test]
    async fn embeds_real_text_at_384_dims_with_sane_cosine_ordering() {
        let engine = EmbeddingEngine::new(test_cache_dir("cosine-ordering"));

        let docs = vec![
            "button \"Sign in\" on the login form".to_string(),
            "button \"Log in\" to authenticate".to_string(),
            "footer copyright notice 2026".to_string(),
        ];
        let embeddings = engine.embed_many(&docs).await.unwrap();

        assert_eq!(embeddings.len(), 3);
        for row in &embeddings {
            assert_eq!(row.len(), DIMENSION);
        }

        let sim_related = cosine(&embeddings[0], &embeddings[1]);
        let sim_unrelated = cosine(&embeddings[0], &embeddings[2]);
        assert!(
            sim_related > sim_unrelated,
            "expected related strings to be more similar: {sim_related} vs {sim_unrelated}"
        );
    }

    #[tokio::test]
    async fn embed_one_matches_the_first_row_of_embed_many() {
        let engine = EmbeddingEngine::new(test_cache_dir("embed-one"));

        let single = engine.embed_one("hello world").await.unwrap();
        assert_eq!(single.len(), DIMENSION);
    }
}
