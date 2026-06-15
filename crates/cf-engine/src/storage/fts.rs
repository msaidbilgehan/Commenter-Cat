//! FTS5 keyword search over comment bodies (Idea §6; task 4.4).
//!
//! A standalone FTS5 index whose `rowid` is the comment id, derived from the
//! comment facts in `index.db`. Search returns comment ids ranked by BM25
//! relevance (best first) — one of the two inputs to hybrid retrieval (§6, 4.7).

use rusqlite::{params, Connection};

use cf_core::error::{CfError, CfResult};

/// Creates the FTS5 virtual table if absent (FTS5 is in bundled SQLite, §10).
///
/// # Errors
/// Returns [`CfError::Storage`] on a DDL error.
pub fn create(conn: &Connection) -> CfResult<()> {
    conn.execute_batch("CREATE VIRTUAL TABLE IF NOT EXISTS comments_fts USING fts5(raw_text);")
        .map_err(|e| CfError::storage("creating FTS5 index").caused_by(e))
}

/// Indexes a comment's text under its id.
///
/// # Errors
/// Returns [`CfError::Storage`] on a write error.
pub fn index(conn: &Connection, comment_id: i64, raw_text: &str) -> CfResult<()> {
    conn.execute(
        "INSERT INTO comments_fts(rowid, raw_text) VALUES (?1, ?2)",
        params![comment_id, raw_text],
    )
    .map_err(|e| CfError::storage("indexing comment for FTS").caused_by(e))?;
    Ok(())
}

/// Keyword search → comment ids, most relevant first (BM25), capped at `limit`.
/// `query` is an FTS5 match expression.
///
/// # Errors
/// Returns [`CfError::Storage`] on a query error.
pub fn search(conn: &Connection, query: &str, limit: usize) -> CfResult<Vec<i64>> {
    let mut stmt = conn
        .prepare(
            "SELECT rowid FROM comments_fts
             WHERE comments_fts MATCH ?1 ORDER BY rank LIMIT ?2",
        )
        .map_err(|e| CfError::storage("preparing FTS query").caused_by(e))?;
    let rows = stmt
        .query_map(params![query, limit as i64], |row| row.get::<_, i64>(0))
        .map_err(|e| CfError::storage("running FTS query").caused_by(e))?;
    let mut ids = Vec::new();
    for row in rows {
        ids.push(row.map_err(|e| CfError::storage("reading FTS row").caused_by(e))?);
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::index_db::IndexDb;
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

    #[test]
    fn test_keyword_search_returns_matching_comments_ranked() {
        let db = IndexDb::open_in_memory().unwrap();
        let parse_id = db
            .insert_comment(&comment("parse the config file"))
            .unwrap();
        let retry_id = db
            .insert_comment(&comment("retry the network request"))
            .unwrap();
        let config_id = db
            .insert_comment(&comment("the config loader reads config"))
            .unwrap();

        create(db.conn()).unwrap();
        for (id, text) in db.comment_texts().unwrap() {
            index(db.conn(), id, &text).unwrap();
        }

        let hits = search(db.conn(), "config", 10).unwrap();
        assert!(hits.contains(&parse_id) && hits.contains(&config_id));
        assert!(!hits.contains(&retry_id), "non-matching comment excluded");
        // The comment mentioning "config" twice ranks above the single mention.
        assert_eq!(hits.first(), Some(&config_id));
    }

    #[test]
    fn test_limit_caps_results() {
        let db = IndexDb::open_in_memory().unwrap();
        for _ in 0..5 {
            db.insert_comment(&comment("config config config")).unwrap();
        }
        create(db.conn()).unwrap();
        for (id, text) in db.comment_texts().unwrap() {
            index(db.conn(), id, &text).unwrap();
        }
        assert_eq!(search(db.conn(), "config", 3).unwrap().len(), 3);
    }
}
