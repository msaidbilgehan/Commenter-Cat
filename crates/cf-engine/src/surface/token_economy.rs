//! The token economy (Idea §4a) — the first-class constraint.
//!
//! No agent-facing return is a firehose. Every result is **ranked, bounded, and
//! drillable**: summaries by default (the ranked head plus a total count), a
//! budget-aware cap that returns the highest-priority slice that *fits*, an
//! explicit `truncated` label whenever the view is partial, and a `cursor` to
//! drill further. Bound code is never bundled in — it is fetched on demand via
//! `context`.

use serde::{Deserialize, Serialize};

/// A drill cursor into a ranked result set — an opaque offset the caller passes
/// back to fetch the next page (Idea §4a).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    /// The index into the ranked set at which the next page begins.
    pub offset: usize,
}

/// A budget: a maximum item count and/or a maximum token estimate. Either, both,
/// or neither — an empty budget returns everything (still labeled).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Budget {
    /// Hard cap on the number of items returned.
    pub limit: Option<usize>,
    /// Soft cap on the estimated token cost of the returned slice.
    pub max_tokens: Option<usize>,
}

impl Budget {
    /// A budget with only an item limit.
    #[must_use]
    pub const fn limit(limit: usize) -> Self {
        Self {
            limit: Some(limit),
            max_tokens: None,
        }
    }

    /// A budget with only a token cap.
    #[must_use]
    pub const fn tokens(max_tokens: usize) -> Self {
        Self {
            limit: None,
            max_tokens: Some(max_tokens),
        }
    }
}

/// A bounded, ranked, drillable view over a result set (Idea §4a). Always
/// labeled: `total` is the full count even when `items` is a partial slice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundedView<T> {
    /// The highest-priority slice that fit the budget.
    pub items: Vec<T>,
    /// The total number of ranked items available (the ranked head's count).
    pub total: usize,
    /// Whether `items` is a partial view (some ranked items were withheld).
    pub truncated: bool,
    /// The cursor to fetch the next page, present iff `truncated`.
    pub cursor: Option<Cursor>,
}

impl<T> BoundedView<T> {
    /// The number of items actually returned.
    #[must_use]
    pub fn returned(&self) -> usize {
        self.items.len()
    }
}

/// Bounds a **ranked** list to `budget`, starting at `offset`, returning the
/// highest-priority slice that fits plus a `truncated` flag and drill `cursor`.
///
/// `token_cost` estimates an item's token cost (only consulted when the budget
/// sets `max_tokens`). At least one item past `offset` is always returned when
/// any remain, so a single oversized item cannot stall pagination.
pub fn bound<T>(
    ranked: Vec<T>,
    budget: &Budget,
    offset: usize,
    token_cost: impl Fn(&T) -> usize,
) -> BoundedView<T> {
    let total = ranked.len();
    let start = offset.min(total);
    let mut items = Vec::new();
    let mut tokens = 0usize;

    for item in ranked.into_iter().skip(start) {
        if budget.limit.is_some_and(|limit| items.len() >= limit) {
            break;
        }
        if let Some(max) = budget.max_tokens {
            let cost = token_cost(&item);
            // Guarantee progress: always take at least one item, even if it
            // alone exceeds the cap.
            if !items.is_empty() && tokens + cost > max {
                break;
            }
            tokens += cost;
        }
        items.push(item);
    }

    let consumed = start + items.len();
    let truncated = consumed < total;
    BoundedView {
        items,
        total,
        truncated,
        cursor: truncated.then_some(Cursor { offset: consumed }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nums() -> Vec<u32> {
        (0..10).collect()
    }

    #[test]
    fn test_limit_truncates_with_cursor() {
        let view = bound(nums(), &Budget::limit(3), 0, |_| 1);
        assert_eq!(view.items, vec![0, 1, 2]);
        assert_eq!(view.total, 10);
        assert!(view.truncated);
        assert_eq!(view.cursor, Some(Cursor { offset: 3 }));
    }

    #[test]
    fn test_cursor_drills_to_next_page() {
        let view = bound(nums(), &Budget::limit(3), 3, |_| 1);
        assert_eq!(view.items, vec![3, 4, 5]);
        assert_eq!(view.cursor, Some(Cursor { offset: 6 }));
    }

    #[test]
    fn test_token_cap_returns_slice_that_fits() {
        // Each item "costs" 4 tokens; a 10-token cap fits 2 (8 ≤ 10, 12 > 10).
        let view = bound(nums(), &Budget::tokens(10), 0, |_| 4);
        assert_eq!(view.items, vec![0, 1]);
        assert!(view.truncated);
    }

    #[test]
    fn test_oversized_first_item_still_makes_progress() {
        // One item costs 100, cap is 10 — we still return it (no stall) and the
        // cursor advances.
        let view = bound(nums(), &Budget::tokens(10), 0, |_| 100);
        assert_eq!(view.items, vec![0]);
        assert_eq!(view.cursor, Some(Cursor { offset: 1 }));
    }

    #[test]
    fn test_empty_budget_returns_all_untruncated() {
        let view = bound(nums(), &Budget::default(), 0, |_| 1);
        assert_eq!(view.returned(), 10);
        assert!(!view.truncated);
        assert_eq!(view.cursor, None);
    }

    #[test]
    fn test_offset_past_end_is_empty() {
        let view = bound(nums(), &Budget::limit(5), 99, |_| 1);
        assert!(view.items.is_empty());
        assert!(!view.truncated);
    }
}
