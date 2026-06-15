//! Property tests for the determinism promise (Idea §4, §5, §11).
//!
//! Identity stability: a cosmetic edit (whitespace, case, delimiter, trailing
//! punctuation) preserves the Tier-2 fingerprint, so a suppression is never
//! orphaned; a marker escalation breaks it, because that is a real change.
//! Findings ordering: dedup is order-independent, so the same findings produce a
//! byte-identical report regardless of the order providers ran.

// Test code: unwrap/expect on known-good fixtures is idiomatic.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use cf_core::finding::{dedup_findings, Category, Finding, FindingTarget, Fix, Origin, Range};
use cf_core::identity::fingerprint::cosmetic_fingerprint;
use cf_core::severity::Severity;
use cf_core::symbol::CommentId;
use proptest::prelude::*;

fn finding(rule: &str, category: Category, line: u32) -> Finding {
    Finding {
        file: "a.py".to_owned(),
        target: FindingTarget::Comment(CommentId::new("c")),
        range: Range::new(line, line + 1, line, line),
        origin: Origin::Ruff,
        provider_rule_id: format!("ruff:{rule}"),
        canonical_rule_id: rule.to_owned(),
        category,
        severity: Severity::Warning,
        severity_native: None,
        message: format!("{rule} here"),
        fix: Fix::None,
        url: None,
        also_from: Default::default(),
    }
}

/// A small finding set with a deliberate cross-tool duplicate (same category +
/// overlapping range) to exercise the dedup/merge path.
fn sample_findings() -> Vec<Finding> {
    vec![
        finding("D417", Category::DocDrift, 1),
        finding("D100", Category::DocMissing, 5),
        finding("ERA001", Category::CommentedCode, 9),
        finding("jsdoc", Category::DocDrift, 1), // dup of D417 by (category, range)
    ]
}

proptest! {
    /// Cosmetic edits never change the fingerprint (Tier-2 is stable, Idea §4).
    #[test]
    fn cosmetic_edits_preserve_fingerprint(words in prop::collection::vec("[a-z]{1,8}", 1..6)) {
        let content = words.join(" ");
        let base = cosmetic_fingerprint(&format!("# {content}"));
        // Reflow whitespace, upper-case, swap the delimiter, add trailing punctuation.
        let cosmetic = format!("//   {}  .", content.to_uppercase());
        prop_assert_eq!(cosmetic_fingerprint(&cosmetic), base);
    }

    /// A marker escalation is a real change → the fingerprint must differ.
    #[test]
    fn marker_escalation_breaks_fingerprint(words in prop::collection::vec("[a-z]{1,8}", 1..6)) {
        let content = words.join(" ");
        let base = cosmetic_fingerprint(&format!("# {content}"));
        let escalated = cosmetic_fingerprint(&format!("# FIXME {content}"));
        prop_assert_ne!(escalated, base);
    }

    /// Dedup is order-independent: any permutation of the inputs yields the same
    /// canonical output (so the report is reproducible, Idea §5/§11).
    #[test]
    fn dedup_is_order_independent(order in Just((0..4usize).collect::<Vec<_>>()).prop_shuffle()) {
        let base = sample_findings();
        let permuted: Vec<Finding> = order.iter().map(|&i| base[i].clone()).collect();
        prop_assert_eq!(dedup_findings(base), dedup_findings(permuted));
    }
}
