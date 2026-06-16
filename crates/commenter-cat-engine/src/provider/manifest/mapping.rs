//! Declarative mapping tables (Idea §5; task 6.4).
//!
//! `[severity_map]` and `[category_map]` bridge a tool's native values to the
//! canonical [`Finding`](commenter_cat_core::Finding) with **no embedded code** — pure table
//! lookups (Idea §5). The moment code would be needed is the threshold to a
//! Tier-2 native provider.

use std::collections::BTreeMap;

use commenter_cat_core::finding::Category;
use commenter_cat_core::severity::Severity;

/// Maps a tool's native severity string to canonical via `[severity_map]`.
#[must_use]
pub fn apply_severity_map(
    severity_map: &BTreeMap<String, Severity>,
    native_severity: &str,
) -> Option<Severity> {
    severity_map.get(native_severity).copied()
}

/// Maps a tool's native rule id to a canonical category via `[category_map]`.
#[must_use]
pub fn apply_category_map(
    category_map: &BTreeMap<String, Category>,
    native_rule_id: &str,
) -> Option<Category> {
    category_map.get(native_rule_id).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_severity_map_produces_canonical() {
        let map: BTreeMap<String, Severity> = [
            ("error".to_owned(), Severity::Error),
            ("suggestion".to_owned(), Severity::Info),
        ]
        .into_iter()
        .collect();
        assert_eq!(apply_severity_map(&map, "error"), Some(Severity::Error));
        assert_eq!(apply_severity_map(&map, "suggestion"), Some(Severity::Info));
        assert_eq!(apply_severity_map(&map, "unknown"), None);
    }

    #[test]
    fn test_category_map_produces_canonical() {
        let map: BTreeMap<String, Category> =
            [("Vale.Spelling".to_owned(), Category::CommentStyle)]
                .into_iter()
                .collect();
        assert_eq!(
            apply_category_map(&map, "Vale.Spelling"),
            Some(Category::CommentStyle)
        );
        assert_eq!(apply_category_map(&map, "Vale.Other"), None);
    }
}
