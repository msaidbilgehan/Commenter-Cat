//! sqlite-vec similarity index (Idea §6; task 4.6).
//!
//! The raw vectors live in `inputs.db` (so they survive a rebuild); the queryable
//! `vec0` index is derived here into `index.db`. Nearest-neighbor search returns
//! comment ids nearest first — the semantic input to hybrid retrieval (§6, 4.7).
//! The extension is registered process-wide by [`super::connection`].

use rusqlite::{params, Connection};

use commenter_cat_core::error::{CommenterCatError, CommenterCatResult};

use super::inputs_db::vector_to_blob;

/// Creates the `vec0` virtual table for vectors of `dimensions` floats.
///
/// # Errors
/// Returns [`CommenterCatError::Storage`] on a DDL error.
pub fn create(conn: &Connection, dimensions: usize) -> CommenterCatResult<()> {
    let ddl = format!(
        "CREATE VIRTUAL TABLE IF NOT EXISTS comments_vec USING vec0(embedding float[{dimensions}]);"
    );
    conn.execute_batch(&ddl)
        .map_err(|e| CommenterCatError::storage("creating vec0 index").caused_by(e))
}

/// Indexes a comment's embedding under its id.
///
/// # Errors
/// Returns [`CommenterCatError::Storage`] on a write error.
pub fn index(conn: &Connection, comment_id: i64, vector: &[f32]) -> CommenterCatResult<()> {
    conn.execute(
        "INSERT INTO comments_vec(rowid, embedding) VALUES (?1, ?2)",
        params![comment_id, vector_to_blob(vector)],
    )
    .map_err(|e| CommenterCatError::storage("indexing comment vector").caused_by(e))?;
    Ok(())
}

/// k-nearest-neighbor search → comment ids, nearest first.
///
/// # Errors
/// Returns [`CommenterCatError::Storage`] on a query error.
pub fn knn(conn: &Connection, query: &[f32], k: usize) -> CommenterCatResult<Vec<i64>> {
    let mut stmt = conn
        .prepare(
            "SELECT rowid FROM comments_vec
             WHERE embedding MATCH ?1 AND k = ?2 ORDER BY distance",
        )
        .map_err(|e| CommenterCatError::storage("preparing kNN query").caused_by(e))?;
    let rows = stmt
        .query_map(params![vector_to_blob(query), k as i64], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|e| CommenterCatError::storage("running kNN query").caused_by(e))?;
    let mut ids = Vec::new();
    for row in rows {
        ids.push(row.map_err(|e| CommenterCatError::storage("reading kNN row").caused_by(e))?);
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::{DeterministicEmbedder, Embedder};
    use crate::storage::index_db::IndexDb;
    use commenter_cat_core::comment::Comment;
    use commenter_cat_core::finding::Range;
    use commenter_cat_core::kind::CommentKind;
    use commenter_cat_core::lang::Language;

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

    #[test]
    fn test_knn_returns_nearest_first() {
        let embedder = DeterministicEmbedder;
        let db = IndexDb::open_in_memory().unwrap();
        create(db.conn(), embedder.dimensions()).unwrap();

        let near_id = db
            .insert_comment(&comment("open the database connection"))
            .unwrap();
        let mid_id = db
            .insert_comment(&comment("open the database file"))
            .unwrap();
        let far_id = db
            .insert_comment(&comment("bake a chocolate cake"))
            .unwrap();
        for (id, text) in db.comment_texts().unwrap() {
            index(db.conn(), id, &embedder.embed(&text).unwrap()).unwrap();
        }

        let query = embedder.embed("open the database connection now").unwrap();
        let nearest = knn(db.conn(), &query, 3).unwrap();
        assert_eq!(
            nearest.first(),
            Some(&near_id),
            "exact-ish match is nearest"
        );
        assert_eq!(
            nearest.last(),
            Some(&far_id),
            "unrelated comment is farthest"
        );
        assert!(nearest.contains(&mid_id));
    }
}
