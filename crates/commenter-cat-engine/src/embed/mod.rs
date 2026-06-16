//! Local embeddings (Idea §6, §10, §13; task 4.5).
//!
//! Embeddings are computed **on-device** — comments are proprietary context and
//! never leave the machine (Idea §13 rejects cloud embeddings). The [`Embedder`]
//! trait is the seam; storage and search depend only on it, not on a specific
//! model. Raw vectors are cached in `inputs.db` keyed `(content_hash,
//! model_version)`, so they survive an index rebuild with no re-embed (Idea §6).
//!
//! Two implementations:
//!
//! * [`DeterministicEmbedder`] — a hashing bag-of-words embedder. Deterministic,
//!   offline, and fast; the default for tests and reproducibility, and a working
//!   semantic-ish backend (shared tokens → closer vectors).
//!
//! * [`onnx::OnnxEmbedder`] — the real, local ONNX model via `fastembed` / `ort`
//!   (all-MiniLM-L6-v2, 384-dim). It runs **on-device** and downloads/caches the
//!   model on first use; comments never leave the machine (Idea §6, §13). This is
//!   the production embedder; the deterministic one is the offline test default.

pub mod onnx;

use commenter_cat_core::error::CommenterCatResult;

/// Computes local embedding vectors for text (Idea §6).
pub trait Embedder {
    /// The model identifier — part of the embedding cache key (Idea §6).
    fn model_version(&self) -> &str;

    /// The fixed output dimensionality.
    fn dimensions(&self) -> usize;

    /// Embeds `text` into a unit-length vector of [`Self::dimensions`] floats.
    ///
    /// # Errors
    /// Returns an error if a real model fails to run inference.
    fn embed(&self, text: &str) -> CommenterCatResult<Vec<f32>>;

    /// Embeds a batch of texts. The default runs [`Self::embed`] per text; real
    /// backends override this for throughput (one model invocation per batch).
    ///
    /// # Errors
    /// Returns an error if inference fails for any text.
    fn embed_batch(&self, texts: &[&str]) -> CommenterCatResult<Vec<Vec<f32>>> {
        texts.iter().map(|text| self.embed(text)).collect()
    }
}

/// Output dimensionality of the deterministic embedder.
const DETERMINISTIC_DIM: usize = 64;

/// Model id of the deterministic embedder (its cache-key namespace).
const DETERMINISTIC_MODEL: &str = "deterministic-hash-v1";

/// A deterministic, offline embedder: tokens are hashed into buckets of a
/// fixed-dimension vector, which is then L2-normalized. Identical text always
/// yields an identical vector, and texts sharing tokens are closer — enough for
/// reproducible storage/search tests without a downloaded model.
#[derive(Debug, Default, Clone, Copy)]
pub struct DeterministicEmbedder;

impl Embedder for DeterministicEmbedder {
    fn model_version(&self) -> &str {
        DETERMINISTIC_MODEL
    }

    fn dimensions(&self) -> usize {
        DETERMINISTIC_DIM
    }

    fn embed(&self, text: &str) -> CommenterCatResult<Vec<f32>> {
        let mut vector = vec![0.0_f32; DETERMINISTIC_DIM];
        for token in tokenize(text) {
            let bucket = (fnv1a(&token) as usize) % DETERMINISTIC_DIM;
            vector[bucket] += 1.0;
        }
        l2_normalize(&mut vector);
        Ok(vector)
    }
}

/// Lowercase alphanumeric tokens.
fn tokenize(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_lowercase)
}

/// FNV-1a 32-bit — a stable hash (not the std default, which is not portable).
fn fnv1a(text: &str) -> u32 {
    let mut hash = 0x811c_9dc5_u32;
    for byte in text.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// Scales a vector to unit length in place (no-op for the zero vector).
fn l2_normalize(vector: &mut [f32]) {
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > 0.0 {
        for value in vector.iter_mut() {
            *value /= norm;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::inputs_db::InputsDb;

    fn cosine(a: &[f32], b: &[f32]) -> f32 {
        a.iter().zip(b).map(|(x, y)| x * y).sum()
    }

    #[test]
    fn test_deterministic_and_correct_shape() {
        let embedder = DeterministicEmbedder;
        let a = embedder.embed("the cat sat on the mat").unwrap();
        let b = embedder.embed("the cat sat on the mat").unwrap();
        assert_eq!(a, b, "identical text → identical vector");
        assert_eq!(a.len(), embedder.dimensions());
    }

    #[test]
    fn test_similar_text_is_closer_than_unrelated() {
        let embedder = DeterministicEmbedder;
        let base = embedder.embed("parse the config file").unwrap();
        let similar = embedder.embed("parse the config value").unwrap();
        let unrelated = embedder.embed("quantum chromodynamics lattice").unwrap();
        assert!(
            cosine(&base, &similar) > cosine(&base, &unrelated),
            "shared tokens must yield a higher cosine"
        );
    }

    #[test]
    fn test_persisted_and_retrieved_by_key() {
        // Idea §6: vectors cached by (content_hash, model_version).
        let embedder = DeterministicEmbedder;
        let db = InputsDb::open_in_memory().unwrap();
        let text = "explains the retry loop";
        let vector = embedder.embed(text).unwrap();

        db.store_embedding("hash-of-text", embedder.model_version(), &vector)
            .unwrap();
        let retrieved = db
            .embedding("hash-of-text", embedder.model_version())
            .unwrap();
        assert_eq!(retrieved, Some(vector));
    }
}
