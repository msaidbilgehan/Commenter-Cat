//! The real local ONNX embedder (Idea §6, §13; task 4.5).
//!
//! Computes embeddings **on-device** with `fastembed` / `ort` (ONNX Runtime), so
//! comments — proprietary context — never leave the machine (Idea §13 rejects
//! cloud embeddings). The model (sentence-transformers/all-MiniLM-L6-v2, 384-dim)
//! is downloaded and cached on first use, keyed into `inputs.db` by
//! [`OnnxEmbedder::model_version`].
//!
//! `fastembed`'s model holds mutable inference state and `embed` takes
//! `&mut self`, so it is wrapped in a `Mutex` to satisfy the shared-reference
//! [`Embedder`] trait while staying usable across threads.

use std::path::PathBuf;
use std::sync::Mutex;

use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

use cf_core::error::{CfError, CfResult};

use super::Embedder;

/// The bundled model — sentence-transformers/all-MiniLM-L6-v2.
const MODEL: EmbeddingModel = EmbeddingModel::AllMiniLML6V2;

/// Cache-key namespace for this model + revision (part of the embedding key,
/// Idea §6).
const MODEL_VERSION: &str = "all-minilm-l6-v2";

/// The model's output dimensionality.
const DIMENSIONS: usize = 384;

/// A local ONNX text embedder backed by `fastembed`.
pub struct OnnxEmbedder {
    model: Mutex<TextEmbedding>,
}

impl OnnxEmbedder {
    /// Loads the model into fastembed's default cache, downloading it on first
    /// use.
    ///
    /// # Errors
    /// Returns [`CfError::Storage`] if the model cannot be fetched or loaded.
    pub fn new() -> CfResult<Self> {
        Self::load(InitOptions::new(MODEL).with_show_download_progress(false))
    }

    /// Loads the model into a specific cache directory (e.g. the project cache).
    ///
    /// # Errors
    /// Returns [`CfError::Storage`] if the model cannot be fetched or loaded.
    pub fn with_cache_dir(cache_dir: PathBuf) -> CfResult<Self> {
        Self::load(
            InitOptions::new(MODEL)
                .with_show_download_progress(false)
                .with_cache_dir(cache_dir),
        )
    }

    fn load(options: InitOptions) -> CfResult<Self> {
        let model = TextEmbedding::try_new(options)
            .map_err(|e| CfError::storage(format!("loading ONNX embedding model: {e}")))?;
        Ok(Self {
            model: Mutex::new(model),
        })
    }
}

impl Embedder for OnnxEmbedder {
    fn model_version(&self) -> &str {
        MODEL_VERSION
    }

    fn dimensions(&self) -> usize {
        DIMENSIONS
    }

    fn embed(&self, text: &str) -> CfResult<Vec<f32>> {
        let mut vectors = self.embed_batch(&[text])?;
        vectors
            .pop()
            .ok_or_else(|| CfError::storage("ONNX model returned no embedding"))
    }

    fn embed_batch(&self, texts: &[&str]) -> CfResult<Vec<Vec<f32>>> {
        let mut model = self
            .model
            .lock()
            .map_err(|_| CfError::storage("embedding model lock poisoned"))?;
        model
            .embed(texts, None)
            .map_err(|e| CfError::storage(format!("ONNX inference failed: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cosine(a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        let norm_a = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b = b.iter().map(|x| x * x).sum::<f32>().sqrt();
        dot / (norm_a * norm_b)
    }

    // Heavy + networked: downloads the ONNX model on first run. Excluded from the
    // fast offline unit suite; run with `cargo test -- --ignored`.
    #[test]
    #[ignore = "downloads the all-MiniLM-L6-v2 ONNX model (~90 MB)"]
    fn test_real_onnx_embeddings() {
        let embedder = OnnxEmbedder::new().expect("model loads");
        assert_eq!(embedder.dimensions(), DIMENSIONS);

        let base = embedder.embed("parse the configuration file").unwrap();
        assert_eq!(base.len(), DIMENSIONS);

        // Deterministic for identical input.
        assert_eq!(
            embedder.embed("parse the configuration file").unwrap(),
            base
        );

        // The real model places similar text closer than unrelated text.
        let similar = embedder.embed("read the config settings").unwrap();
        let unrelated = embedder.embed("a recipe for chocolate cake").unwrap();
        assert!(
            cosine(&base, &similar) > cosine(&base, &unrelated),
            "semantically similar text must have a higher cosine"
        );

        // Batch matches per-item embedding up to tiny batch floating-point
        // effects (different batch dimensions → non-bit-identical accumulation).
        let batch = embedder
            .embed_batch(&["parse the configuration file", "read the config settings"])
            .unwrap();
        assert_eq!(batch.len(), 2);
        assert!(cosine(&batch[0], &base) > 0.9999, "batch[0] ≈ single embed");
        assert!(
            cosine(&batch[1], &similar) > 0.9999,
            "batch[1] ≈ single embed"
        );
    }
}
