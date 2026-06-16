//! Actionable-first ranking (Idea §4a, §14).
//!
//! Every agent-facing return is ranked so the most actionable item leads. For
//! findings that means **severity → blame-age → marker weight** (a critical
//! secret outranks a stale TODO; among equals, the older comment leads). For
//! search it means the **FTS/vector score**. Ranking weights are impl-tuned
//! (§14), but the order is a named, tested contract.

use std::cmp::{Ordering, Reverse};

use commenter_cat_core::severity::Severity;

/// The actionable-first priority key. Field order *is* the tie-break order, and
/// the derived `Ord` makes the **largest** `Priority` the most actionable
/// (highest severity, then oldest blame, then heaviest marker) — so ranking
/// sorts descending.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Priority {
    /// Primary: higher severity is more actionable (`Critical` is greatest).
    pub severity: Severity,
    /// Secondary: older comments lead — a larger age sorts greater.
    pub blame_age_secs: i64,
    /// Tertiary: heavier markers (e.g. `DO_NOT_MERGE`) lead.
    pub marker_weight: u32,
}

impl Priority {
    /// Builds a priority key.
    #[must_use]
    pub const fn new(severity: Severity, blame_age_secs: i64, marker_weight: u32) -> Self {
        Self {
            severity,
            blame_age_secs,
            marker_weight,
        }
    }
}

/// Ranks items actionable-first (descending priority). Stable: equal-priority
/// items keep their input order, so an already-canonical input stays canonical.
pub fn rank_by<T>(items: &mut [T], key: impl Fn(&T) -> Priority) {
    items.sort_by_key(|item| Reverse(key(item)));
}

/// Ranks items by a descending search score (FTS/vector). NaN scores sort last.
pub fn rank_by_score<T>(items: &mut [T], score: impl Fn(&T) -> f64) {
    items.sort_by(|a, b| match score(b).partial_cmp(&score(a)) {
        Some(ordering) => ordering,
        // Push NaN (unscored) to the end rather than panicking.
        None => score(a).is_nan().cmp(&score(b).is_nan()),
    });
}

/// Orders two priorities most-actionable first (for callers that compare without
/// sorting a slice).
#[must_use]
pub fn cmp_actionable(a: &Priority, b: &Priority) -> Ordering {
    b.cmp(a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_severity_leads_then_blame_age_then_marker() {
        let mut items = vec![
            ("stale_todo", Priority::new(Severity::Warning, 1_000, 1)),
            ("secret", Priority::new(Severity::Critical, 0, 0)),
            ("fresh_warning", Priority::new(Severity::Warning, 10, 1)),
            ("heavy_marker", Priority::new(Severity::Warning, 1_000, 5)),
        ];
        rank_by(&mut items, |(_, p)| *p);
        let order: Vec<&str> = items.iter().map(|(name, _)| *name).collect();
        assert_eq!(
            order,
            vec!["secret", "heavy_marker", "stale_todo", "fresh_warning"],
            "critical first; among warnings, older then heavier-marker leads"
        );
    }

    #[test]
    fn test_rank_by_score_descending_nan_last() {
        let mut items = vec![("low", 0.1), ("high", 0.9), ("nan", f64::NAN), ("mid", 0.5)];
        rank_by_score(&mut items, |(_, s)| *s);
        let order: Vec<&str> = items.iter().map(|(name, _)| *name).collect();
        assert_eq!(order, vec!["high", "mid", "low", "nan"]);
    }
}
