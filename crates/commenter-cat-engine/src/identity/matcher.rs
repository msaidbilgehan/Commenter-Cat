//! Tiered cross-scan matching (Idea §4; task 5.3).
//!
//! Precision is **per use-case** (Idea §4):
//!
//! | Tier | Match | For |
//! |---|---|---|
//! | 1 Exact | `content_hash` equal | cache hits |
//! | 2 Cosmetic | same `(bound_symbol, fingerprint)` | suppression / baseline |
//! | 3 Relocated | same `fingerprint`, different `bound_symbol` | following a refactor |
//! | 4 Reworded | same `(bound_symbol, kind)`, similarity ≥ τ, no rival | issue / blame **only** |
//!
//! The fuzzy Tier 4 is **architecturally forbidden from suppression** (Idea §4,
//! risk R5): a false fuzzy match there would hide a real finding. It is offered
//! only for issue/blame continuity, and only when exactly one candidate crosses
//! the threshold (no rival).

use commenter_cat_core::kind::CommentKind;

/// τ — the Tier-4 fuzzy similarity threshold (Idea §4: ≈0.8). A named constant,
/// not a magic number (general.md `ORG_MAGIC_NUMBER`).
pub const FUZZY_SIMILARITY_THRESHOLD: f32 = 0.8;

/// What a match will be used for — gates the fuzzy tier (Idea §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchUseCase {
    /// Suppression / baseline. Tier 4 is **forbidden** (a false match hides a
    /// finding).
    Suppression,
    /// Issue / blame continuity. Tier 4 is allowed.
    IssueContinuity,
}

/// Which tier produced a match (Idea §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchTier {
    /// Exact `content_hash`.
    Exact,
    /// Same `(bound_symbol, fingerprint)`.
    Cosmetic,
    /// Same `fingerprint`, different `bound_symbol`.
    Relocated,
    /// Fuzzy: same `(bound_symbol, kind)`, similarity ≥ τ, no rival.
    Reworded,
}

/// A candidate comment from a prior scan.
#[derive(Debug, Clone)]
pub struct Candidate {
    /// The comment id.
    pub id: i64,
    /// Exact-bytes content hash (Tier 1).
    pub content_hash: String,
    /// The bound symbol (`None` for an orphan).
    pub bound_symbol: Option<String>,
    /// The comment kind.
    pub kind: CommentKind,
    /// The cosmetic fingerprint (Tiers 2–3).
    pub cosmetic_fingerprint: String,
    /// The embedding vector (Tier 4 similarity).
    pub vector: Vec<f32>,
}

/// A resolved match: which candidate, via which tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Match {
    /// The matched candidate's id.
    pub id: i64,
    /// The tier that resolved it.
    pub tier: MatchTier,
}

/// Matches `query` against `candidates` for `use_case` (Idea §4).
#[must_use]
pub fn match_comment(
    query: &Candidate,
    candidates: &[Candidate],
    use_case: MatchUseCase,
) -> Option<Match> {
    // Tier 1 — exact content hash.
    if let Some(c) = candidates
        .iter()
        .find(|c| c.content_hash == query.content_hash)
    {
        return Some(Match {
            id: c.id,
            tier: MatchTier::Exact,
        });
    }
    // Tier 2 — cosmetic: same bound_symbol + fingerprint.
    if let Some(c) = candidates.iter().find(|c| {
        c.bound_symbol == query.bound_symbol && c.cosmetic_fingerprint == query.cosmetic_fingerprint
    }) {
        return Some(Match {
            id: c.id,
            tier: MatchTier::Cosmetic,
        });
    }
    // Tier 3 — relocated: same fingerprint, different bound_symbol.
    if let Some(c) = candidates.iter().find(|c| {
        c.cosmetic_fingerprint == query.cosmetic_fingerprint && c.bound_symbol != query.bound_symbol
    }) {
        return Some(Match {
            id: c.id,
            tier: MatchTier::Relocated,
        });
    }
    // Tier 4 — reworded: issue/blame continuity ONLY, never suppression (R5).
    if use_case == MatchUseCase::IssueContinuity {
        return fuzzy_match(query, candidates);
    }
    None
}

/// Tier 4: among same-`(bound_symbol, kind)` candidates, return the single one
/// crossing τ. Zero crossings → no match; two or more → a rival → no match.
fn fuzzy_match(query: &Candidate, candidates: &[Candidate]) -> Option<Match> {
    let mut crossing: Vec<i64> = candidates
        .iter()
        .filter(|c| c.bound_symbol == query.bound_symbol && c.kind == query.kind)
        .filter(|c| cosine(&query.vector, &c.vector) >= FUZZY_SIMILARITY_THRESHOLD)
        .map(|c| c.id)
        .collect();
    match crossing.len() {
        1 => Some(Match {
            id: crossing.remove(0),
            tier: MatchTier::Reworded,
        }),
        _ => None,
    }
}

/// Cosine similarity; `0.0` if either vector is degenerate.
fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embed::{DeterministicEmbedder, Embedder};

    fn candidate(id: i64, hash: &str, symbol: &str, fingerprint: &str, text: &str) -> Candidate {
        Candidate {
            id,
            content_hash: hash.to_owned(),
            bound_symbol: Some(symbol.to_owned()),
            kind: CommentKind::Line,
            cosmetic_fingerprint: fingerprint.to_owned(),
            vector: DeterministicEmbedder.embed(text).unwrap(),
        }
    }

    #[test]
    fn test_tier1_exact_content_hash() {
        let query = candidate(0, "H1", "m.f", "FP1", "x");
        let candidates = vec![candidate(1, "H1", "m.f", "FP1", "x")];
        let m = match_comment(&query, &candidates, MatchUseCase::Suppression).unwrap();
        assert_eq!(
            m,
            Match {
                id: 1,
                tier: MatchTier::Exact
            }
        );
    }

    #[test]
    fn test_tier2_cosmetic_for_suppression() {
        // Whitespace edit → new content hash, same symbol + fingerprint.
        let query = candidate(0, "H2", "m.f", "FP1", "x");
        let candidates = vec![candidate(1, "H1", "m.f", "FP1", "x")];
        let m = match_comment(&query, &candidates, MatchUseCase::Suppression).unwrap();
        assert_eq!(
            m,
            Match {
                id: 1,
                tier: MatchTier::Cosmetic
            }
        );
    }

    #[test]
    fn test_tier3_relocated() {
        // Same fingerprint, different bound symbol (followed through a refactor).
        let query = candidate(0, "H2", "m.g", "FP1", "x");
        let candidates = vec![candidate(1, "H1", "m.f", "FP1", "x")];
        let m = match_comment(&query, &candidates, MatchUseCase::IssueContinuity).unwrap();
        assert_eq!(
            m,
            Match {
                id: 1,
                tier: MatchTier::Relocated
            }
        );
    }

    #[test]
    fn test_tier4_reworded_issue_only_never_suppression() {
        // Same symbol + kind, different fingerprint, high semantic similarity.
        let query = candidate(0, "Hq", "m.f", "FPnew", "explains the retry loop here");
        let candidates = vec![candidate(
            1,
            "Hc",
            "m.f",
            "FPold",
            "explains the retry loop",
        )];

        // Allowed for issue continuity.
        let m = match_comment(&query, &candidates, MatchUseCase::IssueContinuity).unwrap();
        assert_eq!(
            m,
            Match {
                id: 1,
                tier: MatchTier::Reworded
            }
        );
        // Forbidden for suppression (R5).
        assert_eq!(
            match_comment(&query, &candidates, MatchUseCase::Suppression),
            None
        );
    }

    #[test]
    fn test_tier4_rejects_rival() {
        // Two similar candidates in the same scope → ambiguous → no match.
        let query = candidate(0, "Hq", "m.f", "FPnew", "explains the retry loop here");
        let candidates = vec![
            candidate(1, "Hc", "m.f", "FPold", "explains the retry loop"),
            candidate(2, "Hd", "m.f", "FPold2", "explains the retry loop again"),
        ];
        assert_eq!(
            match_comment(&query, &candidates, MatchUseCase::IssueContinuity),
            None
        );
    }

    #[test]
    fn test_no_match_returns_none() {
        let query = candidate(0, "Hq", "m.f", "FPnew", "totally unrelated text");
        let candidates = vec![candidate(1, "Hc", "m.other", "FPold", "different scope")];
        assert_eq!(
            match_comment(&query, &candidates, MatchUseCase::IssueContinuity),
            None
        );
    }
}
