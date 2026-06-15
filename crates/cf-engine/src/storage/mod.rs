//! The two-layer SQLite cache (Idea §6).
//!
//! Storage splits along *cost to produce*, which is what makes "rebuild over
//! migrate" cheap (Idea §6, §11):
//!
//! * [`inputs_db`] — content-addressed, expensive to produce (provider
//!   subprocesses + ONNX inference), keyed by *content* so it survives schema
//!   bumps.
//! * [`index_db`] — the derived, cheap-to-rebuild query layer (comment facts,
//!   FTS5, the `vec0` index, identity/suppression), CF-schema-versioned.
//!
//! [`connection`] opens every database with WAL + sqlite-vec; the engine is the
//! sole writer throughout.

pub mod cache;
pub mod connection;
pub mod fts;
pub mod hashing;
pub mod identity_store;
pub mod index_db;
pub mod inputs_db;
pub mod location;
pub mod rebuild;
pub mod schema;
pub mod vec_index;
