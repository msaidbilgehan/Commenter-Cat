//! Severity resolution (Idea §5).
//!
//! A provider's native severity is reconciled to Commenter-Cat's canonical 4-level scale by
//! a three-tier rule, **first hit wins**:
//!
//! 1. **Config override** — `[severity]` by `provider_rule_id`, bare
//!    `canonical_rule_id`, `category`, or `origin` (most specific first).
//! 2. **Category canonical default** — the consistency anchor
//!    ([`Category::canonical_severity`]); guarantees `doc_drift` is `error`
//!    whether `ruff` or `eslint` found it.
//! 3. **Per-tool translation** ([`per_tool_severity`]) — the tool's native scale
//!    mapped to canonical, the fallback for a category with no anchor.
//!
//! Every category currently anchors (tier 2 always resolves), so tier 3 is
//! dormant-by-design but kept correct and tested for future categories and for
//! interpreting a tool's recorded `severity_native`.

use std::collections::BTreeMap;

use crate::severity::Severity;

use super::category::Category;
use super::Origin;

/// Resolves a finding's canonical severity via tiers 1–2 (Idea §5).
///
/// `overrides` is the resolved `[severity]` map (rule id / category / origin →
/// level). The tool's raw native severity is recorded separately as the
/// finding's `severity_native`; its canonical interpretation, if ever needed as
/// a tier-3 fallback, comes from [`per_tool_severity`].
#[must_use]
pub fn resolve_severity(
    category: Category,
    provider_rule_id: &str,
    canonical_rule_id: &str,
    origin: &Origin,
    overrides: &BTreeMap<String, Severity>,
) -> Severity {
    // Tier 1: config override, most specific key first.
    for key in [
        provider_rule_id,
        canonical_rule_id,
        category.as_str(),
        origin.as_str(),
    ] {
        if let Some(&severity) = overrides.get(key) {
            return severity;
        }
    }
    // Tier 2: the category anchor (cross-language consistency). Tier 3
    // (per_tool_severity) is the documented fallback for a category with no
    // anchor — none exist today, so the anchor is authoritative.
    category.canonical_severity()
}

/// Maps a tool's native severity string to canonical (Idea §5 per-tool table).
///
/// Returns `None` when the origin has no native scale (`ruff`, `native`) or is a
/// third-party manifest provider (which declares its own `severity_map`, Phase
/// 6), or when the token is unrecognized.
#[must_use]
pub fn per_tool_severity(origin: &Origin, native: &str) -> Option<Severity> {
    match origin {
        // eslint: 2 = error, 1 = warn (numeric in `-f json`; words accepted too).
        Origin::Eslint => match native.trim() {
            "2" | "error" => Some(Severity::Error),
            "1" | "warn" | "warning" => Some(Severity::Warning),
            _ => None,
        },
        // shellcheck: error / warning / info / style → error / warning / info / info.
        Origin::Shellcheck => match native.trim() {
            "error" => Some(Severity::Error),
            "warning" => Some(Severity::Warning),
            "info" | "style" => Some(Severity::Info),
            _ => None,
        },
        // gitleaks: a hit is always critical (Idea §5).
        Origin::Gitleaks => Some(Severity::Critical),
        // No native scale → resolved by category.
        Origin::Ruff | Origin::Native => None,
        // Manifest providers map severity declaratively in their manifest.
        Origin::Other(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn overrides(pairs: &[(&str, Severity)]) -> BTreeMap<String, Severity> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect()
    }

    #[test]
    fn test_category_anchor_consistent_across_origins() {
        // doc_drift resolves to error whether ruff or eslint reported it (Idea §5).
        let empty = overrides(&[]);
        let from_ruff = resolve_severity(
            Category::DocDrift,
            "ruff:D417",
            "D417",
            &Origin::Ruff,
            &empty,
        );
        let from_eslint = resolve_severity(
            Category::DocDrift,
            "eslint:jsdoc/check-param-names",
            "check-param-names",
            &Origin::Eslint,
            &empty,
        );
        assert_eq!(from_ruff, Severity::Error);
        assert_eq!(from_eslint, Severity::Error);
    }

    #[test]
    fn test_config_override_beats_category() {
        let ov = overrides(&[("doc_missing", Severity::Error)]);
        // Category anchor for doc_missing is warning; the override wins.
        let resolved = resolve_severity(
            Category::DocMissing,
            "ruff:D100",
            "D100",
            &Origin::Ruff,
            &ov,
        );
        assert_eq!(resolved, Severity::Error);
    }

    #[test]
    fn test_rule_override_beats_category_override() {
        let ov = overrides(&[
            ("ruff:D417", Severity::Critical),
            ("doc_drift", Severity::Info),
        ]);
        // The most specific key (full provider_rule_id) wins.
        let resolved =
            resolve_severity(Category::DocDrift, "ruff:D417", "D417", &Origin::Ruff, &ov);
        assert_eq!(resolved, Severity::Critical);
    }

    #[test]
    fn test_origin_level_override_is_least_specific() {
        let ov = overrides(&[("eslint", Severity::Info)]);
        let resolved = resolve_severity(
            Category::DocDrift,
            "eslint:check-param-names",
            "check-param-names",
            &Origin::Eslint,
            &ov,
        );
        assert_eq!(resolved, Severity::Info);
    }

    #[test]
    fn test_category_beats_per_tool() {
        // A comment_style finding from eslint with native "2" (which the per-tool
        // table maps to error) must still resolve to the category anchor: info.
        let empty = overrides(&[]);
        let resolved = resolve_severity(
            Category::CommentStyle,
            "eslint:some-rule",
            "some-rule",
            &Origin::Eslint,
            &empty,
        );
        assert_eq!(
            resolved,
            Severity::Info,
            "category anchor wins over per-tool"
        );
        assert_eq!(
            per_tool_severity(&Origin::Eslint, "2"),
            Some(Severity::Error)
        );
    }

    #[test]
    fn test_per_tool_table() {
        assert_eq!(
            per_tool_severity(&Origin::Eslint, "2"),
            Some(Severity::Error)
        );
        assert_eq!(
            per_tool_severity(&Origin::Eslint, "1"),
            Some(Severity::Warning)
        );
        assert_eq!(
            per_tool_severity(&Origin::Shellcheck, "error"),
            Some(Severity::Error)
        );
        assert_eq!(
            per_tool_severity(&Origin::Shellcheck, "style"),
            Some(Severity::Info)
        );
        assert_eq!(
            per_tool_severity(&Origin::Shellcheck, "info"),
            Some(Severity::Info)
        );
        assert_eq!(
            per_tool_severity(&Origin::Gitleaks, "whatever"),
            Some(Severity::Critical)
        );
        assert_eq!(per_tool_severity(&Origin::Ruff, "anything"), None);
        assert_eq!(per_tool_severity(&Origin::Native, "anything"), None);
        assert_eq!(
            per_tool_severity(&Origin::Other("vale".into()), "error"),
            None
        );
    }
}
