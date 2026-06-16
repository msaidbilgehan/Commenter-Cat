//! Index persistence + the read session (Idea §6, §4a; completes task 7.1's
//! "persist into the index").
//!
//! `commenter-cat check` fuses the unified records in memory; [`persist`] writes them into
//! the two-layer cache — comments + findings + cross-scan identity + the FTS and
//! semantic indexes — so the find/understand/update verbs (`query`, `context`,
//! `apply_edit`, `remove`) resolve against a real index. The derived `index.db`
//! is rebuilt fresh each scan; `inputs.db`'s embedding cache persists, so a
//! re-scan reuses vectors with no re-embed (Idea §6). A [`Session`] is the
//! read/update handle the verbs share.

use std::path::{Path, PathBuf};

use commenter_cat_core::comment::Comment;
use commenter_cat_core::error::{CommenterCatError, CommenterCatResult};
use commenter_cat_core::identity::assign_identities;

use crate::embed::{DeterministicEmbedder, Embedder};
use crate::search::HybridSearch;
use crate::storage::index_db::IndexDb;
use crate::storage::inputs_db::InputsDb;
use crate::storage::{fts, identity_store, location, vec_index};

/// The embedder the CLI/MCP use for the search index. The deterministic backend
/// keeps `commenter-cat check`/`commenter-cat query` offline + instant and aids reproducibility (Idea
/// §11); switching to the config's `Local` ONNX backend is a one-line change
/// here (the cached vectors in `inputs.db` are keyed by `model_version`, so the
/// two never collide).
#[must_use]
pub fn default_embedder() -> DeterministicEmbedder {
    DeterministicEmbedder
}

/// Persists `comments` (with their fused findings) into the two-layer cache.
///
/// The `index.db` is rebuilt from scratch (the derived layer is disposable);
/// embedding vectors are reused from `inputs.db` on a content-hash hit, embedded
/// + cached on a miss (Idea §6 — rebuild without re-embed).
///
/// # Errors
/// Returns [`CommenterCatError::Storage`] on any cache write failure.
pub fn persist<E: Embedder>(
    repo_root: &Path,
    comments: &[Comment],
    embedder: &E,
) -> CommenterCatResult<()> {
    location::ensure_cache_dir(repo_root)?;
    let inputs = InputsDb::open(&location::inputs_db_path(repo_root))?;

    let index_path = location::index_db_path(repo_root);
    // The derived index is rebuilt fresh; remove any stale build first.
    if index_path.exists() {
        std::fs::remove_file(&index_path).map_err(|e| {
            CommenterCatError::storage(format!("removing stale index {}", index_path.display()))
                .caused_by(e)
        })?;
    }
    let index = IndexDb::open(&index_path)?;
    fts::create(index.conn())?;
    vec_index::create(index.conn(), embedder.dimensions())?;

    let identities = assign_identities(comments);
    for (comment, identity) in comments.iter().zip(&identities) {
        let id = index.insert_comment(comment)?;
        fts::index(index.conn(), id, &comment.raw_text)?;
        let vector = reuse_or_embed(&inputs, embedder, &comment.content_hash, &comment.raw_text)?;
        vec_index::index(index.conn(), id, &vector)?;
        for finding in &comment.findings {
            index.insert_finding(id, finding)?;
        }
        identity_store::store(&index, id, identity)?;
    }
    Ok(())
}

/// Returns the cached vector for `content_hash`, or embeds + caches on a miss.
fn reuse_or_embed<E: Embedder>(
    inputs: &InputsDb,
    embedder: &E,
    content_hash: &str,
    text: &str,
) -> CommenterCatResult<Vec<f32>> {
    if let Some(cached) = inputs.embedding(content_hash, embedder.model_version())? {
        return Ok(cached);
    }
    let computed = embedder.embed(text)?;
    inputs.store_embedding(content_hash, embedder.model_version(), &computed)?;
    Ok(computed)
}

/// A read/update handle over a persisted `index.db` — the engine entry point the
/// index-backed verbs share (Idea §4a).
pub struct Session {
    repo_root: PathBuf,
    index: IndexDb,
}

impl Session {
    /// Opens the session for `repo_root`, or errors if no index has been built.
    ///
    /// # Errors
    /// Returns [`CommenterCatError::Storage`] if `index.db` is absent (run `commenter-cat check`) or
    /// cannot be opened.
    pub fn open(repo_root: &Path) -> CommenterCatResult<Session> {
        let index_path = location::index_db_path(repo_root);
        if !index_path.exists() {
            return Err(CommenterCatError::storage(
                "no index found — run `commenter-cat check` to build it before query/context/apply",
            ));
        }
        Ok(Session {
            repo_root: repo_root.to_path_buf(),
            index: IndexDb::open(&index_path)?,
        })
    }

    /// Searches comments by text (FTS + semantic, RRF-fused), returning the
    /// matched `(id, comment)` pairs in ranked order, capped at `limit`.
    ///
    /// # Errors
    /// Returns [`commenter_cat_core::CommenterCatError`] on a search or read error.
    pub fn query(&self, text: &str, limit: usize) -> CommenterCatResult<Vec<(i64, Comment)>> {
        let embedder = default_embedder();
        let ids = HybridSearch::new(&self.index, &embedder).query(text, limit)?;
        let mut hits = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(comment) = self.index.comment(id)? {
                hits.push((id, comment));
            }
        }
        Ok(hits)
    }

    /// Fetches a comment by its index id.
    ///
    /// # Errors
    /// Returns [`commenter_cat_core::CommenterCatError`] on a read error.
    pub fn comment(&self, id: i64) -> CommenterCatResult<Option<Comment>> {
        self.index.comment(id)
    }

    /// Reads the on-disk source of a comment's file.
    ///
    /// # Errors
    /// Returns [`CommenterCatError::Storage`] if the file cannot be read.
    pub fn read_source(&self, comment: &Comment) -> CommenterCatResult<String> {
        let path = self.file_path(comment);
        std::fs::read_to_string(&path).map_err(|e| {
            CommenterCatError::storage(format!("reading {}", path.display())).caused_by(e)
        })
    }

    /// Writes new source back to a comment's file (after a safe edit).
    ///
    /// # Errors
    /// Returns [`CommenterCatError::Storage`] if the file cannot be written.
    pub fn write_source(&self, comment: &Comment, source: &str) -> CommenterCatResult<()> {
        let path = self.file_path(comment);
        std::fs::write(&path, source).map_err(|e| {
            CommenterCatError::storage(format!("writing {}", path.display())).caused_by(e)
        })
    }

    /// The repository root the session was opened for.
    #[must_use]
    pub fn repo_root(&self) -> &Path {
        &self.repo_root
    }

    /// The absolute path of a comment's (repo-relative) file.
    fn file_path(&self, comment: &Comment) -> PathBuf {
        self.repo_root.join(&comment.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::check::check;
    use crate::provider::RuleProvider;
    use crate::testutil::TestRepo;
    use commenter_cat_core::config::ResolvedConfig;

    const NO_PROVIDERS: [&dyn RuleProvider; 0] = [];

    /// Builds + persists an index for a one-file repo, returning the repo.
    fn indexed_repo() -> TestRepo {
        let repo = TestRepo::new();
        repo.write(
            "pkg/cache.py",
            "# TODO retry the database connection\nx = 1\n",
        );
        let result = check(
            repo.path(),
            &ResolvedConfig::default(),
            &NO_PROVIDERS,
            false,
        )
        .unwrap();
        persist(repo.path(), &result.comments, &default_embedder()).unwrap();
        repo
    }

    #[test]
    fn test_persist_then_query_round_trips() {
        let repo = indexed_repo();
        let session = Session::open(repo.path()).unwrap();

        // The persisted comment is findable by its text …
        let hits = session.query("database connection", 10).unwrap();
        assert!(
            !hits.is_empty(),
            "the comment is retrievable from the index"
        );
        let (id, comment) = &hits[0];
        assert!(comment.raw_text.contains("database connection"));

        // … fetchable by id, carrying its fused native finding …
        let fetched = session.comment(*id).unwrap().unwrap();
        assert!(fetched.findings.iter().any(|f| f.message.contains("TODO")));
        // … with its cosmetic fingerprint persisted (identity is real).
        assert!(fetched.cosmetic_fingerprint.is_some());
    }

    #[test]
    fn test_open_without_index_errors() {
        let repo = TestRepo::new();
        assert!(Session::open(repo.path()).is_err());
    }
}
