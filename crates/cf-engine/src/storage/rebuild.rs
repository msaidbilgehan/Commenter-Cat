//! Rebuild over migrate (Idea §6, §11; task 4.8).
//!
//! On an `index.db` schema mismatch (or an explicit rebuild), the derived index
//! is re-built from `inputs.db` by the deterministic native pass — **never
//! migrated in place**. Comment facts are re-derived (ripgrep-class), while
//! provider results and embedding vectors are reused from `inputs.db` with **no
//! provider re-run and no re-embed** (Idea §6). Because the pass is
//! deterministic, the re-derive is reproducible — a rebuild loses nothing.

use std::path::Path;

use cf_core::comment::Comment;
use cf_core::error::CfResult;
use cf_core::version::INDEX_DB_SCHEMA_VERSION;

use crate::embed::Embedder;
use crate::storage::index_db::IndexDb;
use crate::storage::inputs_db::InputsDb;
use crate::storage::{connection, fts, vec_index};

const INDEX_VERSION_KEY: &str = "index_schema_version";

/// Derives a fresh in-memory index from comment facts and the cached vectors in
/// `inputs` (re-embedding only on a cache miss).
///
/// # Errors
/// Returns [`cf_core::CfError`] on any storage error.
pub fn derive_index<E: Embedder>(
    inputs: &InputsDb,
    comments: &[Comment],
    embedder: &E,
) -> CfResult<IndexDb> {
    let index = IndexDb::open_in_memory()?;
    derive_into(&index, inputs, comments, embedder)?;
    Ok(index)
}

/// Derives comment facts, the FTS index, and the vec index into `index`, reusing
/// cached vectors from `inputs` — the rebuild-without-re-embed path (Idea §6).
///
/// # Errors
/// Returns [`cf_core::CfError`] on any storage error.
pub fn derive_into<E: Embedder>(
    index: &IndexDb,
    inputs: &InputsDb,
    comments: &[Comment],
    embedder: &E,
) -> CfResult<()> {
    fts::create(index.conn())?;
    vec_index::create(index.conn(), embedder.dimensions())?;
    for comment in comments {
        let id = index.insert_comment(comment)?;
        fts::index(index.conn(), id, &comment.raw_text)?;
        let vector = reuse_or_embed(inputs, embedder, &comment.content_hash, &comment.raw_text)?;
        vec_index::index(index.conn(), id, &vector)?;
    }
    Ok(())
}

/// Returns the cached vector for `content_hash`, or embeds + caches on a miss.
fn reuse_or_embed<E: Embedder>(
    inputs: &InputsDb,
    embedder: &E,
    content_hash: &str,
    text: &str,
) -> CfResult<Vec<f32>> {
    if let Some(cached) = inputs.embedding(content_hash, embedder.model_version())? {
        return Ok(cached);
    }
    let computed = embedder.embed(text)?;
    inputs.store_embedding(content_hash, embedder.model_version(), &computed)?;
    Ok(computed)
}

/// Whether the `index.db` at `path` must be rebuilt: missing, unreadable, or its
/// recorded schema version differs from the current one (Idea §6/§11).
#[must_use]
pub fn needs_rebuild(path: &Path) -> bool {
    if !path.exists() {
        return true;
    }
    match recorded_index_version(path) {
        Some(version) => version != INDEX_DB_SCHEMA_VERSION,
        None => true,
    }
}

/// Reads the recorded index schema version without resetting it (a raw read).
fn recorded_index_version(path: &Path) -> Option<u32> {
    let conn = connection::open(path).ok()?;
    let raw: String = conn
        .query_row(
            "SELECT value FROM meta WHERE key = ?1",
            rusqlite::params![INDEX_VERSION_KEY],
            |row| row.get(0),
        )
        .ok()?;
    raw.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::DeterministicEmbedder;
    use crate::search::HybridSearch;
    use cf_core::finding::Range;
    use cf_core::kind::CommentKind;
    use cf_core::lang::Language;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// An embedder that counts how many times it actually embeds.
    struct CountingEmbedder {
        inner: DeterministicEmbedder,
        embeds: AtomicUsize,
    }

    impl CountingEmbedder {
        fn new() -> Self {
            Self {
                inner: DeterministicEmbedder,
                embeds: AtomicUsize::new(0),
            }
        }
        fn count(&self) -> usize {
            self.embeds.load(Ordering::Relaxed)
        }
    }

    impl Embedder for CountingEmbedder {
        fn model_version(&self) -> &str {
            self.inner.model_version()
        }
        fn dimensions(&self) -> usize {
            self.inner.dimensions()
        }
        fn embed(&self, text: &str) -> CfResult<Vec<f32>> {
            self.embeds.fetch_add(1, Ordering::Relaxed);
            self.inner.embed(text)
        }
    }

    fn comment(hash: &str, text: &str) -> Comment {
        Comment::new(
            "a.py",
            hash,
            Language::Python,
            CommentKind::Line,
            Range::new(0, 1, 1, 1),
            text,
        )
    }

    fn sample() -> Vec<Comment> {
        vec![
            comment("h1", "parse the config file"),
            comment("h2", "retry the request"),
            comment("h3", "open the database connection"),
        ]
    }

    #[test]
    fn test_rebuild_reuses_cached_vectors_with_zero_reembed() {
        let embedder = CountingEmbedder::new();
        let inputs = InputsDb::open_in_memory().unwrap();
        let comments = sample();

        // A prior scan embedded + cached each vector (the expensive step).
        for comment in &comments {
            let vector = embedder.embed(&comment.raw_text).unwrap();
            inputs
                .store_embedding(&comment.content_hash, embedder.model_version(), &vector)
                .unwrap();
        }
        let after_warm = embedder.count();
        assert_eq!(after_warm, comments.len());

        // The index-schema rebuild re-derives from inputs.db: zero re-embed.
        let index = derive_index(&inputs, &comments, &embedder).unwrap();
        assert_eq!(embedder.count(), after_warm, "rebuild must not re-embed");
        assert_eq!(index.comment_texts().unwrap().len(), comments.len());
    }

    #[test]
    fn test_rebuild_is_deterministic() {
        let embedder = DeterministicEmbedder;
        let inputs = InputsDb::open_in_memory().unwrap();
        let comments = sample();

        let first = derive_index(&inputs, &comments, &embedder).unwrap();
        let second = derive_index(&inputs, &comments, &embedder).unwrap();
        let q1 = HybridSearch::new(&first, &embedder)
            .query("config database", 10)
            .unwrap();
        let q2 = HybridSearch::new(&second, &embedder)
            .query("config database", 10)
            .unwrap();
        assert_eq!(q1, q2, "deterministic re-derive yields identical results");
    }

    #[test]
    fn test_needs_rebuild_detects_version_mismatch() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("index.db");

        {
            let index = IndexDb::open(&path).unwrap();
            index.set_meta(INDEX_VERSION_KEY, "0").unwrap();
        }
        assert!(needs_rebuild(&path), "a stale version forces a rebuild");

        {
            let index = IndexDb::open(&path).unwrap();
            index
                .set_meta(INDEX_VERSION_KEY, &INDEX_DB_SCHEMA_VERSION.to_string())
                .unwrap();
        }
        assert!(
            !needs_rebuild(&path),
            "the current version needs no rebuild"
        );

        assert!(
            needs_rebuild(&dir.path().join("missing.db")),
            "a missing index forces a rebuild"
        );
    }
}
