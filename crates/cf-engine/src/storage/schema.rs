//! `index.db` relational schema (Idea §4, §6; task 4.2).
//!
//! The derived, CF-schema-versioned query layer. Comment facts are stored as
//! indexed columns (for filtering) plus a `record_json` blob (for a lossless
//! round-trip of the §4 record); findings live in a denormalized table for fast
//! category/severity filtering; identity and suppression tables are created here
//! but populated in Phases 5 and 7. FTS5 and the `vec0` index are added by their
//! own modules ([`super::fts`], [`super::vec_index`]).

/// DDL for the base `index.db` schema.
pub const INDEX_SCHEMA: &str = "\
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS comments (
    id                   INTEGER PRIMARY KEY,
    path                 TEXT NOT NULL,
    content_hash         TEXT NOT NULL,
    language             TEXT NOT NULL,
    kind                 TEXT NOT NULL,
    start_byte           INTEGER NOT NULL,
    end_byte             INTEGER NOT NULL,
    start_line           INTEGER NOT NULL,
    end_line             INTEGER NOT NULL,
    raw_text             TEXT NOT NULL,
    bound_symbol         TEXT,
    is_rot_candidate     INTEGER NOT NULL,
    cosmetic_fingerprint TEXT,
    record_json          TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_comments_path ON comments(path);
CREATE INDEX IF NOT EXISTS idx_comments_content_hash ON comments(content_hash);

CREATE TABLE IF NOT EXISTS findings (
    id                INTEGER PRIMARY KEY,
    comment_id        INTEGER NOT NULL REFERENCES comments(id) ON DELETE CASCADE,
    origin            TEXT NOT NULL,
    provider_rule_id  TEXT NOT NULL,
    canonical_rule_id TEXT NOT NULL,
    category          TEXT NOT NULL,
    severity          TEXT NOT NULL,
    finding_json      TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_findings_comment ON findings(comment_id);
CREATE INDEX IF NOT EXISTS idx_findings_category ON findings(category);

-- Populated in Phase 5 (identity) / Phase 7 (suppression).
CREATE TABLE IF NOT EXISTS identity (
    comment_id           INTEGER PRIMARY KEY REFERENCES comments(id) ON DELETE CASCADE,
    bound_symbol         TEXT,
    kind                 TEXT NOT NULL,
    cosmetic_fingerprint TEXT NOT NULL,
    ordinal              INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_identity_base
    ON identity(bound_symbol, kind, cosmetic_fingerprint);

CREATE TABLE IF NOT EXISTS suppressions (
    id                   INTEGER PRIMARY KEY,
    bound_symbol         TEXT,
    cosmetic_fingerprint TEXT NOT NULL,
    rule                 TEXT,
    reason               TEXT
);
";

/// Tables dropped and re-created during a rebuild (everything derived).
pub const INDEX_TABLES: [&str; 5] = ["suppressions", "identity", "findings", "comments", "meta"];
