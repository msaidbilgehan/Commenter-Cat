//! Hybrid retrieval (Idea §6; task 4.7).
//!
//! Fuses FTS5 keyword search and sqlite-vec semantic search into one ranked
//! comment list via reciprocal rank fusion ([`rrf`]). This feeds the `query`
//! find verb (Idea §4a). Each signal fetches a wider candidate pool than the
//! final `limit` so fusion has material to work with.

pub mod rrf;

use cf_core::error::CfResult;

use crate::embed::Embedder;
use crate::storage::index_db::IndexDb;
use crate::storage::{fts, vec_index};

/// How many candidates each signal fetches per requested result, so RRF has a
/// wide enough pool to fuse.
const CANDIDATE_FACTOR: usize = 5;

/// A hybrid keyword+semantic searcher over one `index.db`.
pub struct HybridSearch<'a, E: Embedder> {
    index: &'a IndexDb,
    embedder: &'a E,
}

impl<'a, E: Embedder> HybridSearch<'a, E> {
    /// Binds a searcher to an index and embedder.
    pub fn new(index: &'a IndexDb, embedder: &'a E) -> Self {
        Self { index, embedder }
    }

    /// Returns comment ids most relevant to `text`, fused and capped at `limit`.
    ///
    /// # Errors
    /// Returns [`cf_core::CfError`] on a storage/query error.
    pub fn query(&self, text: &str, limit: usize) -> CfResult<Vec<i64>> {
        let pool = limit.saturating_mul(CANDIDATE_FACTOR).max(limit);

        let keyword = match build_fts_query(text) {
            Some(query) => fts::search(self.index.conn(), &query, pool)?,
            None => Vec::new(),
        };
        let query_vector = self.embedder.embed(text)?;
        let semantic = vec_index::knn(self.index.conn(), &query_vector, pool)?;

        Ok(rrf::reciprocal_rank_fusion(&[keyword, semantic], limit))
    }
}

/// Builds a lenient FTS5 match expression from free text: each alphanumeric term
/// is quoted (so special characters cannot break the query) and OR-joined.
/// Returns `None` when there is no usable term.
fn build_fts_query(text: &str) -> Option<String> {
    let terms: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .map(|term| format!("\"{term}\""))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" OR "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::DeterministicEmbedder;
    use cf_core::comment::Comment;
    use cf_core::finding::Range;
    use cf_core::kind::CommentKind;
    use cf_core::lang::Language;

    fn comment(text: &str) -> Comment {
        Comment::new(
            "a.py",
            "h",
            Language::Python,
            CommentKind::Line,
            Range::new(0, 1, 1, 1),
            text,
        )
    }

    fn build_index(texts: &[&str], embedder: &DeterministicEmbedder) -> (IndexDb, Vec<i64>) {
        let db = IndexDb::open_in_memory().unwrap();
        fts::create(db.conn()).unwrap();
        vec_index::create(db.conn(), embedder.dimensions()).unwrap();
        let mut ids = Vec::new();
        for text in texts {
            ids.push(db.insert_comment(&comment(text)).unwrap());
        }
        for (id, text) in db.comment_texts().unwrap() {
            fts::index(db.conn(), id, &text).unwrap();
            vec_index::index(db.conn(), id, &embedder.embed(&text).unwrap()).unwrap();
        }
        (db, ids)
    }

    #[test]
    fn test_hybrid_query_ranks_relevant_first() {
        let embedder = DeterministicEmbedder;
        let (db, ids) = build_index(
            &[
                "parse the config file",
                "retry network request",
                "bake a cake",
            ],
            &embedder,
        );
        let search = HybridSearch::new(&db, &embedder);

        let results = search.query("parse config", 3).unwrap();
        assert_eq!(
            results.first(),
            Some(&ids[0]),
            "the config comment ranks first"
        );
        assert!(!results.is_empty());
    }

    #[test]
    fn test_query_is_deterministic() {
        let embedder = DeterministicEmbedder;
        let (db, _) = build_index(&["alpha beta", "beta gamma", "gamma delta"], &embedder);
        let search = HybridSearch::new(&db, &embedder);
        assert_eq!(
            search.query("beta", 5).unwrap(),
            search.query("beta", 5).unwrap()
        );
    }
}
