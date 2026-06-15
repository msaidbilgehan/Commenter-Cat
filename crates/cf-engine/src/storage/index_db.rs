//! The derived query index, `index.db` (Idea §6; task 4.2).
//!
//! Built by the deterministic native pass reading from `inputs.db`; rebuilt
//! (never migrated) on a schema bump. The engine is the sole writer.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

use cf_core::comment::Comment;
use cf_core::error::{CfError, CfResult};
use cf_core::finding::Finding;
use cf_core::version::INDEX_DB_SCHEMA_VERSION;

use super::connection;
use super::schema::INDEX_SCHEMA;

const META_SCHEMA_VERSION: &str = "index_schema_version";

/// The derived comment-fact / finding / search index.
pub struct IndexDb {
    conn: Connection,
}

impl IndexDb {
    /// Opens (creating + initializing) `index.db` at `path`.
    ///
    /// # Errors
    /// Returns [`CfError::Storage`] on open or schema-creation failure.
    pub fn open(path: &Path) -> CfResult<Self> {
        Self::init(connection::open(path)?)
    }

    /// Opens an in-memory index (tests / ephemeral derivations).
    ///
    /// # Errors
    /// Returns [`CfError::Storage`] on open or schema-creation failure.
    pub fn open_in_memory() -> CfResult<Self> {
        Self::init(connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> CfResult<Self> {
        conn.execute_batch(INDEX_SCHEMA)
            .map_err(|e| CfError::storage("creating index.db schema").caused_by(e))?;
        // Record the version only if absent, so a stale index keeps its recorded
        // version for rebuild detection (Idea §6/§11: rebuild over migrate).
        conn.execute(
            "INSERT OR IGNORE INTO meta(key, value) VALUES (?1, ?2)",
            params![META_SCHEMA_VERSION, INDEX_DB_SCHEMA_VERSION.to_string()],
        )
        .map_err(|e| CfError::storage("recording index.db schema version").caused_by(e))?;
        Ok(Self { conn })
    }

    /// The underlying connection, for the FTS / vec / rebuild modules.
    pub(crate) fn conn(&self) -> &Connection {
        &self.conn
    }

    /// The recorded `index.db` schema version.
    ///
    /// # Errors
    /// Returns [`CfError::Storage`] on a read error or malformed value.
    pub fn schema_version(&self) -> CfResult<u32> {
        let raw = self
            .meta(META_SCHEMA_VERSION)?
            .ok_or_else(|| CfError::storage("index.db missing schema version"))?;
        raw.parse().map_err(|e| {
            CfError::storage(format!("invalid index.db schema version {raw:?}")).caused_by(e)
        })
    }

    /// Reads a meta value.
    ///
    /// # Errors
    /// Returns [`CfError::Storage`] on a read error.
    pub fn meta(&self, key: &str) -> CfResult<Option<String>> {
        self.conn
            .query_row(
                "SELECT value FROM meta WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| CfError::storage(format!("reading meta {key:?}")).caused_by(e))
    }

    /// Writes a meta value.
    ///
    /// # Errors
    /// Returns [`CfError::Storage`] on a write error.
    pub fn set_meta(&self, key: &str, value: &str) -> CfResult<()> {
        self.conn
            .execute(
                "INSERT INTO meta(key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map_err(|e| CfError::storage(format!("writing meta {key:?}")).caused_by(e))?;
        Ok(())
    }

    /// Inserts a comment record, returning its row id (the comment id used by
    /// the FTS / vec indexes and findings).
    ///
    /// # Errors
    /// Returns [`CfError::Storage`] on serialization or write failure.
    pub fn insert_comment(&self, comment: &Comment) -> CfResult<i64> {
        let record_json = serde_json::to_string(comment)
            .map_err(|e| CfError::storage("serializing comment").caused_by(e))?;
        self.conn
            .execute(
                "INSERT INTO comments
                 (path, content_hash, language, kind, start_byte, end_byte, start_line, end_line,
                  raw_text, bound_symbol, is_rot_candidate, cosmetic_fingerprint, record_json)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
                params![
                    comment.path,
                    comment.content_hash,
                    comment.language.as_str(),
                    comment.kind.as_str(),
                    comment.range.start_byte,
                    comment.range.end_byte,
                    comment.range.start_line,
                    comment.range.end_line,
                    comment.raw_text,
                    comment
                        .bound_symbol
                        .as_ref()
                        .map(cf_core::BoundSymbol::as_str),
                    comment.is_rot_candidate,
                    comment.cosmetic_fingerprint,
                    record_json,
                ],
            )
            .map_err(|e| CfError::storage("inserting comment").caused_by(e))?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Reads back a full comment record by id (lossless via `record_json`).
    ///
    /// # Errors
    /// Returns [`CfError::Storage`] on a read or deserialization error.
    pub fn comment(&self, id: i64) -> CfResult<Option<Comment>> {
        let json: Option<String> = self
            .conn
            .query_row(
                "SELECT record_json FROM comments WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| CfError::storage("reading comment").caused_by(e))?;
        match json {
            Some(raw) => {
                Ok(Some(serde_json::from_str(&raw).map_err(|e| {
                    CfError::storage("deserializing comment").caused_by(e)
                })?))
            }
            None => Ok(None),
        }
    }

    /// All `(id, raw_text)` pairs, for deriving the FTS / vec indexes.
    ///
    /// # Errors
    /// Returns [`CfError::Storage`] on a read error.
    pub fn comment_texts(&self) -> CfResult<Vec<(i64, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, raw_text FROM comments ORDER BY id")
            .map_err(|e| CfError::storage("preparing comment scan").caused_by(e))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| CfError::storage("scanning comments").caused_by(e))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| CfError::storage("reading comment row").caused_by(e))?);
        }
        Ok(out)
    }

    /// Attaches a normalized finding to a comment (Idea §5).
    ///
    /// # Errors
    /// Returns [`CfError::Storage`] on serialization or write failure.
    pub fn insert_finding(&self, comment_id: i64, finding: &Finding) -> CfResult<()> {
        let finding_json = serde_json::to_string(finding)
            .map_err(|e| CfError::storage("serializing finding").caused_by(e))?;
        self.conn
            .execute(
                "INSERT INTO findings
                 (comment_id, origin, provider_rule_id, canonical_rule_id, category, severity, finding_json)
                 VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![
                    comment_id,
                    finding.origin.as_str(),
                    finding.provider_rule_id,
                    finding.canonical_rule_id,
                    finding.category.as_str(),
                    finding.severity.as_str(),
                    finding_json,
                ],
            )
            .map_err(|e| CfError::storage("inserting finding").caused_by(e))?;
        Ok(())
    }

    /// The findings attached to a comment.
    ///
    /// # Errors
    /// Returns [`CfError::Storage`] on a read or deserialization error.
    pub fn findings_for_comment(&self, comment_id: i64) -> CfResult<Vec<Finding>> {
        let mut stmt = self
            .conn
            .prepare("SELECT finding_json FROM findings WHERE comment_id = ?1 ORDER BY id")
            .map_err(|e| CfError::storage("preparing findings query").caused_by(e))?;
        let rows = stmt
            .query_map(params![comment_id], |row| row.get::<_, String>(0))
            .map_err(|e| CfError::storage("querying findings").caused_by(e))?;
        let mut out = Vec::new();
        for row in rows {
            let raw = row.map_err(|e| CfError::storage("reading finding row").caused_by(e))?;
            out.push(
                serde_json::from_str(&raw)
                    .map_err(|e| CfError::storage("deserializing finding").caused_by(e))?,
            );
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cf_core::finding::{Category, FindingTarget, Fix, Origin, Range};
    use cf_core::kind::CommentKind;
    use cf_core::lang::Language;
    use cf_core::severity::Severity;
    use cf_core::symbol::{BoundSymbol, CommentId};

    fn sample_comment() -> Comment {
        let mut comment = Comment::new(
            "src/app.py",
            "abc123",
            Language::Python,
            CommentKind::Docstring,
            Range::new(0, 20, 1, 2),
            "\"\"\"docs\"\"\"",
        );
        comment.bound_symbol = Some(BoundSymbol::new("app.main"));
        comment.bound_node_range = Some(Range::new(0, 80, 1, 5));
        comment.markers = vec!["TODO".to_owned()];
        comment.is_rot_candidate = true;
        comment
    }

    #[test]
    fn test_comment_record_round_trips() {
        let db = IndexDb::open_in_memory().unwrap();
        let original = sample_comment();
        let id = db.insert_comment(&original).unwrap();
        assert_eq!(db.comment(id).unwrap(), Some(original));
        assert_eq!(db.comment(9999).unwrap(), None);
    }

    #[test]
    fn test_schema_version_recorded() {
        let db = IndexDb::open_in_memory().unwrap();
        assert_eq!(db.schema_version().unwrap(), INDEX_DB_SCHEMA_VERSION);
    }

    #[test]
    fn test_findings_attach_and_read_back() {
        let db = IndexDb::open_in_memory().unwrap();
        let id = db.insert_comment(&sample_comment()).unwrap();
        let finding = Finding {
            file: "src/app.py".to_owned(),
            target: FindingTarget::Comment(CommentId::new("c0")),
            range: Range::new(0, 20, 1, 1),
            origin: Origin::Ruff,
            provider_rule_id: "ruff:D417".to_owned(),
            canonical_rule_id: "D417".to_owned(),
            category: Category::DocDrift,
            severity: Severity::Error,
            severity_native: None,
            message: "drift".to_owned(),
            fix: Fix::AgentOnly,
            url: None,
            also_from: Default::default(),
        };
        db.insert_finding(id, &finding).unwrap();
        assert_eq!(db.findings_for_comment(id).unwrap(), vec![finding]);
    }

    #[test]
    fn test_comment_texts_for_derivation() {
        let db = IndexDb::open_in_memory().unwrap();
        let id = db.insert_comment(&sample_comment()).unwrap();
        assert_eq!(
            db.comment_texts().unwrap(),
            vec![(id, "\"\"\"docs\"\"\"".to_owned())]
        );
    }
}
