//! The agent-facing surface (Idea §4a).
//!
//! The verbs return data; this layer governs *how much* and *in what order*. The
//! [`token_economy`] keeps every return ranked-bounded-drillable (the first-class
//! constraint), [`ranking`] defines actionable-first order, and [`roundtrip`]
//! closes the find→update→re-check loop in a single call. Both the CLI (Phase
//! 8.1) and the MCP surface (8.3) build on these — one economy, two interfaces.

pub mod ranking;
pub mod roundtrip;
pub mod token_economy;

pub use ranking::{cmp_actionable, rank_by, rank_by_score, Priority};
pub use token_economy::{bound, BoundedView, Budget, Cursor};
