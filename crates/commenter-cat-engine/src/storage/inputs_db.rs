//! The content-addressed input store, `inputs.db` (Idea §6; task 4.1).
//!
//! This is the layer that costs real time to produce — provider subprocesses and
//! ONNX inference — so it is keyed by **content**, never by Commenter-Cat's layout:
//!
//! * provider results by `(content_hash, provider, provider_version)`, and
//! * embedding vectors by `(content_hash, model_version)`.
//!
//! Because the keys are content hashes, a Commenter-Cat schema change never invalidates it;
//! its own schema is deliberately minimal and rarely bumps (Idea §6, §11). The
//! engine is the sole writer (WAL + `busy_timeout`, via [`super::connection`]).

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};

use commenter_cat_core::error::{CommenterCatError, CommenterCatResult};
use commenter_cat_core::version::INPUTS_DB_SCHEMA_VERSION;

use super::connection;

const SCHEMA: &str = "\
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS provider_results (
    content_hash     TEXT NOT NULL,
    provider         TEXT NOT NULL,
    provider_version TEXT NOT NULL,
    run_state        TEXT NOT NULL,
    findings_json    TEXT NOT NULL,
    PRIMARY KEY (content_hash, provider, provider_version)
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS embeddings (
    content_hash  TEXT NOT NULL,
    model_version TEXT NOT NULL,
    vector        BLOB NOT NULL,
    PRIMARY KEY (content_hash, model_version)
) WITHOUT ROWID;
";

const META_SCHEMA_VERSION: &str = "inputs_schema_version";

/// A cached provider run for one file's content (Idea §5 run-state machine).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedProviderResult {
    /// The provider's run state (`SUCCESS` / `EMPTY` / `PARTIAL` / `SKIPPED`).
    pub run_state: String,
    /// The normalized `findings[]` as JSON.
    pub findings_json: String,
}

/// The content-addressed input store.
pub struct InputsDb {
    conn: Connection,
}

impl InputsDb {
    /// Opens (creating + initializing) `inputs.db` at `path`.
    ///
    /// # Errors
    /// Returns [`CommenterCatError::Storage`] on open or schema-creation failure.
    pub fn open(path: &Path) -> CommenterCatResult<Self> {
        Self::init(connection::open(path)?)
    }

    /// Opens an in-memory input store (tests / ephemeral use).
    ///
    /// # Errors
    /// Returns [`CommenterCatError::Storage`] on open or schema-creation failure.
    pub fn open_in_memory() -> CommenterCatResult<Self> {
        Self::init(connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> CommenterCatResult<Self> {
        conn.execute_batch(SCHEMA)
            .map_err(|e| CommenterCatError::storage("creating inputs.db schema").caused_by(e))?;
        // Record the version only if absent, so an existing store keeps its own
        // (rarely-bumped) version (Idea §6).
        conn.execute(
            "INSERT OR IGNORE INTO meta(key, value) VALUES (?1, ?2)",
            params![META_SCHEMA_VERSION, INPUTS_DB_SCHEMA_VERSION.to_string()],
        )
        .map_err(|e| {
            CommenterCatError::storage("recording inputs.db schema version").caused_by(e)
        })?;
        Ok(Self { conn })
    }

    /// The recorded `inputs.db` schema version.
    ///
    /// # Errors
    /// Returns [`CommenterCatError::Storage`] on a read error.
    pub fn schema_version(&self) -> CommenterCatResult<u32> {
        let raw = self
            .meta(META_SCHEMA_VERSION)?
            .ok_or_else(|| CommenterCatError::storage("inputs.db missing schema version"))?;
        raw.parse().map_err(|e| {
            CommenterCatError::storage(format!("invalid inputs.db schema version {raw:?}"))
                .caused_by(e)
        })
    }

    /// Reads a meta value.
    ///
    /// # Errors
    /// Returns [`CommenterCatError::Storage`] on a read error.
    pub fn meta(&self, key: &str) -> CommenterCatResult<Option<String>> {
        self.conn
            .query_row(
                "SELECT value FROM meta WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| CommenterCatError::storage(format!("reading meta {key:?}")).caused_by(e))
    }

    /// Caches a provider run for a file's content (Idea §6 provider-result cache).
    ///
    /// # Errors
    /// Returns [`CommenterCatError::Storage`] on a write error.
    pub fn store_provider_result(
        &self,
        content_hash: &str,
        provider: &str,
        provider_version: &str,
        result: &CachedProviderResult,
    ) -> CommenterCatResult<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO provider_results
                 (content_hash, provider, provider_version, run_state, findings_json)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    content_hash,
                    provider,
                    provider_version,
                    result.run_state,
                    result.findings_json
                ],
            )
            .map_err(|e| {
                CommenterCatError::storage(format!("caching {provider} result")).caused_by(e)
            })?;
        Ok(())
    }

    /// Looks up a cached provider run — a hit means the provider need not re-run
    /// on this unchanged content (Idea §6, the load-bearing performance lever).
    ///
    /// # Errors
    /// Returns [`CommenterCatError::Storage`] on a read error.
    pub fn provider_result(
        &self,
        content_hash: &str,
        provider: &str,
        provider_version: &str,
    ) -> CommenterCatResult<Option<CachedProviderResult>> {
        self.conn
            .query_row(
                "SELECT run_state, findings_json FROM provider_results
                 WHERE content_hash = ?1 AND provider = ?2 AND provider_version = ?3",
                params![content_hash, provider, provider_version],
                |row| {
                    Ok(CachedProviderResult {
                        run_state: row.get(0)?,
                        findings_json: row.get(1)?,
                    })
                },
            )
            .optional()
            .map_err(|e| {
                CommenterCatError::storage(format!("reading {provider} cache")).caused_by(e)
            })
    }

    /// Caches a comment's embedding vector (Idea §6: vectors survive a rebuild).
    ///
    /// # Errors
    /// Returns [`CommenterCatError::Storage`] on a write error.
    pub fn store_embedding(
        &self,
        content_hash: &str,
        model_version: &str,
        vector: &[f32],
    ) -> CommenterCatResult<()> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO embeddings (content_hash, model_version, vector)
                 VALUES (?1, ?2, ?3)",
                params![content_hash, model_version, vector_to_blob(vector)],
            )
            .map_err(|e| CommenterCatError::storage("caching embedding").caused_by(e))?;
        Ok(())
    }

    /// Looks up a cached embedding — a hit means no re-embed on rebuild (Idea §6).
    ///
    /// # Errors
    /// Returns [`CommenterCatError::Storage`] on a read error.
    pub fn embedding(
        &self,
        content_hash: &str,
        model_version: &str,
    ) -> CommenterCatResult<Option<Vec<f32>>> {
        let blob: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT vector FROM embeddings WHERE content_hash = ?1 AND model_version = ?2",
                params![content_hash, model_version],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| CommenterCatError::storage("reading embedding").caused_by(e))?;
        Ok(blob.map(|bytes| blob_to_vector(&bytes)))
    }
}

/// Serializes a vector as little-endian `f32` bytes (platform-independent).
pub(crate) fn vector_to_blob(vector: &[f32]) -> Vec<u8> {
    vector
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

/// Deserializes a little-endian `f32` byte blob into a vector.
pub(crate) fn blob_to_vector(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(state: &str) -> CachedProviderResult {
        CachedProviderResult {
            run_state: state.to_owned(),
            findings_json: "[]".to_owned(),
        }
    }

    #[test]
    fn test_provider_result_round_trip_keyed() {
        let db = InputsDb::open_in_memory().unwrap();
        db.store_provider_result("hashA", "ruff", "0.14.2", &result("SUCCESS"))
            .unwrap();

        assert_eq!(
            db.provider_result("hashA", "ruff", "0.14.2").unwrap(),
            Some(result("SUCCESS"))
        );
        // A different version is a cache miss (comparability key, Idea §5).
        assert_eq!(db.provider_result("hashA", "ruff", "0.15.0").unwrap(), None);
        // A different content hash is a cache miss.
        assert_eq!(db.provider_result("hashB", "ruff", "0.14.2").unwrap(), None);
    }

    #[test]
    fn test_embedding_round_trip_exact() {
        let db = InputsDb::open_in_memory().unwrap();
        let vector = vec![0.0_f32, 1.5, -2.25, 3.125];
        db.store_embedding("c1", "minilm-v1", &vector).unwrap();
        assert_eq!(db.embedding("c1", "minilm-v1").unwrap(), Some(vector));
        // Different model version → miss.
        assert_eq!(db.embedding("c1", "other-v2").unwrap(), None);
    }

    #[test]
    fn test_survives_reopen_with_recorded_schema_version() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("inputs.db");
        {
            let db = InputsDb::open(&path).unwrap();
            db.store_provider_result("h", "gitleaks", "8.0", &result("EMPTY"))
                .unwrap();
            db.store_embedding("h", "m1", &[1.0, 2.0]).unwrap();
        }
        // Reopening (e.g. after an index.db schema bump elsewhere) keeps the data.
        let db = InputsDb::open(&path).unwrap();
        assert_eq!(db.schema_version().unwrap(), INPUTS_DB_SCHEMA_VERSION);
        assert_eq!(
            db.provider_result("h", "gitleaks", "8.0").unwrap(),
            Some(result("EMPTY"))
        );
        assert_eq!(db.embedding("h", "m1").unwrap(), Some(vec![1.0, 2.0]));
    }
}
