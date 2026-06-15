//! Operations — the engine's verbs (Idea §5).
//!
//! The engine is a **conductor**: `cf check` orchestrates the providers and
//! fuses their output with native facts; the parse-invariant [`apply`] path lets
//! an agent safely hold the write path; suppression and the committed baseline
//! filter findings at the normalization layer; and `cf fix` delegates provider
//! autofixes. This realizes the "AI proposes, engine guarantees" division.

pub mod apply;
pub mod baseline;
pub mod check;
pub mod fix;
pub mod index;
pub mod normalize;
pub mod provider_cache;
pub mod suppress;
pub mod triage;
