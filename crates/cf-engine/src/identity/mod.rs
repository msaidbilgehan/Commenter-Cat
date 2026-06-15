//! Cross-scan identity matching (Idea §4; Phase 5).
//!
//! The composite identity type and fingerprint live in [`cf_core::identity`];
//! this module hosts the engine-side [`matcher`] that resolves a comment to a
//! prior-scan candidate at the right precision per use-case, and is persisted by
//! [`crate::storage::identity_store`].

pub mod matcher;

pub use matcher::{
    match_comment, Candidate, Match, MatchTier, MatchUseCase, FUZZY_SIMILARITY_THRESHOLD,
};
