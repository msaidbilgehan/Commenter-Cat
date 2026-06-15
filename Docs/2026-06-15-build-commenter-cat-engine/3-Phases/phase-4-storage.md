---
plan_slug: 2026-06-15-build-commenter-cat-engine
phase: 4
name: storage
goal: "Build the two-layer SQLite cache (content-addressed inputs.db + derived index.db), content-hash keying, FTS5 keyword search, local ONNX embeddings, sqlite-vec similarity, hybrid retrieval, and the deterministic rebuild-from-inputs pass."
depends_on_phases: [2, 3]
parallel_safe_with_phases: []
tasks:
  - id: "4.1"
    name: implement-inputs-db
    action: "Create the inputs.db layer in cf-engine via rusqlite (bundled SQLite, WAL mode, load_extension enabled): a content-addressed store keyed (content_hash, provider, provider_version) for provider-result JSON and (content_hash, model_version) for embedding vectors, with INPUTS_DB_SCHEMA_VERSION recorded in a meta table; engine is the sole writer with busy_timeout."
    files: [crates/cf-engine/src/storage/mod.rs, crates/cf-engine/src/storage/inputs_db.rs, crates/cf-engine/src/storage/connection.rs]
    depends_on: []
    parallel_safe: true
    validation: "cargo test -p cf-engine storage::inputs_db passes: insert+lookup of a provider result by (content_hash, provider, version) and survival across a simulated index-schema bump"
    notes: "Idea §6 two-layer cache: inputs.db is expensive-to-produce, survives schema bumps, keyed by content not layout. WAL = concurrent readers + one writer. rusqlite bundled (not system) SQLite (§10)."
  - id: "4.2"
    name: implement-index-db-schema
    action: "Create the index.db layer: relational comment-facts tables (the Idea §4 comment record fields), the mapping table, identity/suppression tables (populated in Phase 5/7), and an INDEX_DB_SCHEMA_VERSION meta row; this is the derived, cheap-to-rebuild, CF-schema-versioned layer."
    files: [crates/cf-engine/src/storage/index_db.rs, crates/cf-engine/src/storage/schema.rs]
    depends_on: ["4.1"]
    parallel_safe: false
    validation: "cargo test -p cf-engine storage::index_db passes: comment-fact upsert + read round-trips a full §4 comment record (sans search indexes)"
    notes: "Idea §6: index.db derived from inputs.db; rebuilt on a schema bump, never migrated. Per-file upsert is why SQLite over DuckDB (§13)."
  - id: "4.3"
    name: implement-content-hash-cache-keys
    action: "Create the cache-correctness layer: key all cache entries on content/blob hash (never path+mtime; mtime is a fast-path hint only), implement the provider-result cache lookup so a provider never re-runs on an unchanged file, and expose a scanner-on-demand path so queries are correct with zero hooks installed."
    files: [crates/cf-engine/src/storage/cache.rs, crates/cf-engine/src/storage/hashing.rs]
    depends_on: ["4.1"]
    parallel_safe: true
    validation: "cargo test -p cf-engine storage::cache passes: a changed byte invalidates the entry, an unchanged file yields a cache hit, and an mtime-only change does not invalidate"
    notes: "Idea §6 Cache correctness: 'mtime is a liar.' The provider-result cache is the load-bearing performance lever (§6/§7). Different file from 4.2 → parallel-safe."
  - id: "4.4"
    name: implement-fts5-search
    action: "Add the FTS5 keyword-search index over comment bodies into index.db, derived from comment facts, with an insert/query API returning ranked comment ids."
    files: [crates/cf-engine/src/storage/fts.rs, crates/cf-engine/src/storage/index_db.rs]
    depends_on: ["4.2"]
    parallel_safe: false
    validation: "cargo test -p cf-engine storage::fts passes: a keyword query returns the expected comments ranked"
    notes: "Idea §6 Search. FTS5 built into bundled SQLite (§10). Shares index_db.rs with 4.2 → sequential."
  - id: "4.5"
    name: implement-local-embeddings
    action: "Create the embeddings module using fastembed-rs / ort (ONNX) to compute comment vectors locally; store raw vectors in inputs.db keyed (content_hash, model_version); never ship comments to an external embedding API."
    files: [crates/cf-engine/src/embed/mod.rs, crates/cf-engine/src/embed/onnx.rs]
    depends_on: ["4.1"]
    parallel_safe: true
    validation: "cargo test -p cf-engine embed:: passes: a deterministic vector is produced for fixed text and persisted/retrieved by (content_hash, model_version)"
    notes: "Idea §6 + §10 + §13: local-only (proprietary context never leaves machine). model_version is part of the embedding key. Different subtree from 4.2/4.4 → parallel-safe with them."
  - id: "4.6"
    name: implement-sqlite-vec-index
    action: "Integrate sqlite-vec: load the per-platform vec0 extension into the bundled SQLite, derive the vec0 similarity index in index.db from the inputs.db vectors, and expose a nearest-neighbor query API."
    files: [crates/cf-engine/src/storage/vec_index.rs, crates/cf-engine/src/storage/connection.rs]
    depends_on: ["4.5", "4.2"]
    parallel_safe: false
    validation: "cargo test -p cf-engine storage::vec_index passes: kNN over inserted vectors returns nearest first; extension loads on the host platform"
    notes: "Idea §6: vectors live in inputs.db (survive), vec0 index derived into index.db. load_extension must be enabled in rusqlite's bundled SQLite (§6 distribution requirement). Touches connection.rs (4.1) → sequential after storage core."
  - id: "4.7"
    name: implement-hybrid-retrieval
    action: "Create the hybrid-retrieval module: reciprocal rank fusion of FTS5 keyword results and sqlite-vec semantic results into one ranked comment list."
    files: [crates/cf-engine/src/search/mod.rs, crates/cf-engine/src/search/rrf.rs]
    depends_on: ["4.4", "4.6"]
    parallel_safe: false
    validation: "cargo test -p cf-engine search:: passes: RRF fuses keyword+semantic rankings with the documented tie-break"
    notes: "Idea §6 Hybrid retrieval. Feeds the `query` find verb (§4a). Depends on both FTS (4.4) and vec (4.6)."
  - id: "4.8"
    name: implement-rebuild-from-inputs
    action: "Create the deterministic rebuild pass: on an index.db schema mismatch (or explicit rebuild), drop and re-derive index.db from inputs.db — re-inserting precomputed provider JSON + vectors into fresh structures with no provider re-run and no re-embed; resolve repo root + cache location at <repo_root>/.comment-finder/ (gitignored), handling worktrees/submodules."
    files: [crates/cf-engine/src/storage/rebuild.rs, crates/cf-engine/src/storage/location.rs]
    depends_on: ["4.7", "4.3"]
    parallel_safe: false
    validation: "cargo test -p cf-engine storage::rebuild passes: an index-schema bump triggers a byte-identical re-derive from inputs.db with zero provider invocations"
    notes: "Idea §6/§11 rebuild-over-migrate — the core lever. Deterministic native pass → identity/suppression re-derive byte-identically. Cache dir gitignored; shared truth is CI (§7)."
---

# Phase 4: Storage

## Goal
Build the persistence and search layer: the two-layer SQLite cache that makes "rebuild over migrate" cheap (content-addressed `inputs.db` that survives schema bumps + derived `index.db` that is rebuilt), content-hash cache keying (the load-bearing performance lever), FTS5 keyword search, local ONNX embeddings, sqlite-vec similarity, hybrid RRF retrieval, and the deterministic rebuild pass. Depends on Phase 2 (comment facts to persist) and Phase 3 (the Finding schema). The engine is the sole writer throughout.

## Tasks

### 4.1 — implement-inputs-db
- **Action:** Content-addressed `inputs.db` (WAL, load_extension on), keyed by content_hash for provider results + vectors.
- **Files:** `crates/cf-engine/src/storage/{mod.rs,inputs_db.rs,connection.rs}`
- **Depends on:** none (within phase)
- **Validation:** insert/lookup by key; survives a simulated index-schema bump.

### 4.2 — implement-index-db-schema
- **Action:** Derived `index.db` with comment-fact, mapping, and identity/suppression tables + schema-version meta.
- **Files:** `crates/cf-engine/src/storage/{index_db.rs,schema.rs}`
- **Depends on:** 4.1
- **Validation:** full §4 comment-record round-trip (sans search indexes).

### 4.3 — implement-content-hash-cache-keys
- **Action:** Content/blob-hash keying, provider-result cache (no re-run on unchanged file), scanner-on-demand correctness.
- **Files:** `crates/cf-engine/src/storage/{cache.rs,hashing.rs}`
- **Depends on:** 4.1 (parallel-safe with 4.2 — different files)
- **Validation:** changed byte invalidates; unchanged hits; mtime-only does not invalidate.

### 4.4 — implement-fts5-search
- **Action:** FTS5 keyword index over comment bodies, derived into index.db.
- **Files:** `crates/cf-engine/src/storage/fts.rs`, `crates/cf-engine/src/storage/index_db.rs`
- **Depends on:** 4.2 (shares index_db.rs)
- **Validation:** keyword query returns expected comments ranked.

### 4.5 — implement-local-embeddings
- **Action:** Local ONNX embeddings (fastembed-rs/ort), vectors in inputs.db keyed (content_hash, model_version).
- **Files:** `crates/cf-engine/src/embed/{mod.rs,onnx.rs}`
- **Depends on:** 4.1 (parallel-safe with 4.2/4.4)
- **Validation:** deterministic vector per fixed text; persisted/retrieved by key.

### 4.6 — implement-sqlite-vec-index
- **Action:** Load vec0 extension; derive similarity index in index.db from inputs.db vectors; kNN API.
- **Files:** `crates/cf-engine/src/storage/vec_index.rs`, `crates/cf-engine/src/storage/connection.rs`
- **Depends on:** 4.5, 4.2
- **Validation:** kNN returns nearest first; extension loads on host.

### 4.7 — implement-hybrid-retrieval
- **Action:** Reciprocal rank fusion of FTS5 + sqlite-vec into one ranked list.
- **Files:** `crates/cf-engine/src/search/{mod.rs,rrf.rs}`
- **Depends on:** 4.4, 4.6
- **Validation:** RRF fuses with documented tie-break.

### 4.8 — implement-rebuild-from-inputs
- **Action:** Deterministic re-derive of index.db from inputs.db on schema mismatch; cache location resolution.
- **Files:** `crates/cf-engine/src/storage/{rebuild.rs,location.rs}`
- **Depends on:** 4.7, 4.3
- **Validation:** index-schema bump → byte-identical re-derive, zero provider invocations.

## Phase Validation
`cargo test -p cf-engine storage:: search:: embed::` all pass; an end-to-end fixture scan persists comment facts + provider-result placeholders + vectors into the two-layer cache, answers a hybrid query, and survives an `index.db` schema bump via a byte-identical rebuild from `inputs.db` with no provider re-run and no re-embed.
