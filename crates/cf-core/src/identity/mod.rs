//! Cross-scan comment identity (Idea §4).
//!
//! `content_hash` is the *cache* key (exact bytes) — too brittle to be identity,
//! since a one-word edit must not orphan a suppression. So identity is the
//! **composite** `(bound_symbol, kind, cosmetic_fingerprint)` with an ordinal
//! tie-breaker. `bound_symbol` anchors location, so identity survives line
//! shifts; the [`fingerprint`] survives cosmetic edits.

pub mod fingerprint;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub use fingerprint::{cosmetic_fingerprint, normalize};

use crate::comment::Comment;
use crate::kind::CommentKind;
use crate::symbol::BoundSymbol;

/// The composite cross-scan identity of a comment (Idea §4).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CommentIdentity {
    /// The symbol the comment binds to (`None` for an orphan); anchors location.
    pub bound_symbol: Option<BoundSymbol>,
    /// The comment kind.
    pub kind: CommentKind,
    /// The cosmetic fingerprint (survives whitespace/case/punctuation edits).
    pub cosmetic_fingerprint: String,
    /// Tie-breaker among comments sharing the triple above (Idea §4: "ordinal
    /// tie-breaks collisions").
    pub ordinal: u32,
}

impl CommentIdentity {
    /// The `(bound_symbol, kind, fingerprint)` triple without the ordinal — the
    /// key used by Tier-2 cosmetic matching (Idea §4).
    #[must_use]
    pub fn base_key(&self) -> (Option<&str>, CommentKind, &str) {
        (
            self.bound_symbol.as_ref().map(BoundSymbol::as_str),
            self.kind,
            &self.cosmetic_fingerprint,
        )
    }
}

/// Computes identities for a list of comments in stable order, assigning ordinal
/// `0, 1, 2…` to comments that share a `(bound_symbol, kind, fingerprint)`
/// triple (Idea §4). The returned vector is index-aligned with `comments`.
#[must_use]
pub fn assign_identities(comments: &[Comment]) -> Vec<CommentIdentity> {
    let mut counters: HashMap<(Option<String>, CommentKind, String), u32> = HashMap::new();
    let mut identities = Vec::with_capacity(comments.len());
    for comment in comments {
        let bound_symbol = comment.bound_symbol.clone();
        let kind = comment.kind;
        let cosmetic_fingerprint = cosmetic_fingerprint(&comment.raw_text);

        let key = (
            bound_symbol.as_ref().map(ToString::to_string),
            kind,
            cosmetic_fingerprint.clone(),
        );
        let counter = counters.entry(key).or_insert(0);
        let ordinal = *counter;
        *counter += 1;

        identities.push(CommentIdentity {
            bound_symbol,
            kind,
            cosmetic_fingerprint,
            ordinal,
        });
    }
    identities
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding::Range;
    use crate::lang::Language;

    fn comment(text: &str, symbol: &str, range: Range) -> Comment {
        let mut c = Comment::new(
            "a.py",
            "h",
            Language::Python,
            CommentKind::Line,
            range,
            text,
        );
        c.bound_symbol = Some(BoundSymbol::new(symbol));
        c
    }

    #[test]
    fn test_ordinal_disambiguates_collisions() {
        // Two identical comments bound to the same symbol → ordinals 0 and 1.
        let comments = vec![
            comment("# TODO: x", "m.f", Range::new(0, 1, 1, 1)),
            comment("# TODO: x", "m.f", Range::new(0, 1, 9, 9)),
        ];
        let identities = assign_identities(&comments);
        assert_eq!(identities[0].ordinal, 0);
        assert_eq!(identities[1].ordinal, 1);
        // The triple is otherwise identical.
        assert_eq!(identities[0].base_key(), identities[1].base_key());
    }

    #[test]
    fn test_identity_survives_line_shift() {
        // Same comment at line 1 vs line 99 → identical identity (line is not in it).
        let early = &assign_identities(&[comment("# note", "m.f", Range::new(0, 1, 1, 1))])[0];
        let late = &assign_identities(&[comment("# note", "m.f", Range::new(500, 501, 99, 99))])[0];
        assert_eq!(early, late);
    }

    #[test]
    fn test_distinct_symbols_are_distinct_identities() {
        let comments = vec![
            comment("# note", "m.f", Range::new(0, 1, 1, 1)),
            comment("# note", "m.g", Range::new(0, 1, 2, 2)),
        ];
        let identities = assign_identities(&comments);
        assert_ne!(identities[0].base_key(), identities[1].base_key());
        assert_eq!(
            identities[0].ordinal, 0,
            "different triples each start at 0"
        );
        assert_eq!(identities[1].ordinal, 0);
    }
}
