//! Same-category dedup and canonical ordering (Idea §5, §7).
//!
//! Two findings of the **same category** whose byte ranges **overlap** are the
//! same issue seen twice (often one tool, sometimes two). They collapse into one:
//! the highest-severity finding is kept as the representative, and the others'
//! origins are unioned into its `also_from` so provenance survives while the
//! representative's lossless `provider_rule_id` is preserved (Idea §5).
//!
//! The output is always in canonical order `(file, start_line, start_byte,
//! provider_rule_id)` — the property that makes two CI runs on identical inputs
//! produce byte-identical reports (Idea §7).

use std::cmp::Ordering;
use std::collections::BTreeMap;

use super::category::Category;
use super::Finding;

/// Deduplicates and canonically orders a list of findings (Idea §5).
///
/// Merging is confined to findings sharing a `(file, category)`; within that, a
/// sweep over byte ranges merges every overlapping cluster.
#[must_use]
pub fn dedup_findings(findings: Vec<Finding>) -> Vec<Finding> {
    // Group by (file, category): only same-category findings can merge.
    let mut groups: BTreeMap<(String, Category), Vec<Finding>> = BTreeMap::new();
    for finding in findings {
        groups
            .entry((finding.file.clone(), finding.category))
            .or_default()
            .push(finding);
    }

    let mut result = Vec::new();
    for (_key, mut group) in groups {
        // Sort by byte span so overlapping findings are adjacent for the sweep.
        group.sort_by_key(|f| (f.range.start_byte, f.range.end_byte));

        let mut iter = group.into_iter();
        let Some(mut current) = iter.next() else {
            continue;
        };
        let mut cluster_end = current.range.end_byte;

        for next in iter {
            if next.range.start_byte < cluster_end {
                // Overlaps the running cluster → merge.
                cluster_end = cluster_end.max(next.range.end_byte);
                current = merge_two(current, next);
            } else {
                result.push(current);
                cluster_end = next.range.end_byte;
                current = next;
            }
        }
        result.push(current);
    }

    // Canonical order — the byte-identical-report guarantee (Idea §7).
    result.sort_by(|a, b| a.canonical_sort_key().cmp(&b.canonical_sort_key()));
    result
}

/// Merges two overlapping same-category findings: the higher-severity one is the
/// representative (ties broken by canonical key for determinism), with the
/// other's origins unioned into `also_from` (Idea §5).
fn merge_two(a: Finding, b: Finding) -> Finding {
    let a_is_representative = match a.severity.cmp(&b.severity) {
        Ordering::Greater => true,
        Ordering::Less => false,
        // Equal severity → deterministic pick by canonical key.
        Ordering::Equal => a.canonical_sort_key() <= b.canonical_sort_key(),
    };
    let (mut representative, other) = if a_is_representative { (a, b) } else { (b, a) };

    representative.also_from.insert(other.origin);
    representative.also_from.extend(other.also_from);
    // Invariant: `also_from` records *additional* origins, never the primary's.
    representative.also_from.remove(&representative.origin);
    representative
}

#[cfg(test)]
mod tests {
    use super::super::{sample_finding, Finding, Origin, Range};
    use super::*;
    use crate::severity::Severity;

    fn finding(
        file: &str,
        category: Category,
        range: Range,
        origin: Origin,
        severity: Severity,
    ) -> Finding {
        let mut f = sample_finding(file, category, range);
        f.provider_rule_id = format!("{}:{}", origin.as_str(), category.as_str());
        f.origin = origin;
        f.severity = severity;
        f
    }

    #[test]
    fn test_overlapping_same_category_merges_to_max_severity() {
        let a = finding(
            "a.py",
            Category::DocStyle,
            Range::new(0, 10, 1, 1),
            Origin::Ruff,
            Severity::Info,
        );
        let b = finding(
            "a.py",
            Category::DocStyle,
            Range::new(5, 15, 1, 1),
            Origin::Eslint,
            Severity::Warning,
        );

        let merged = dedup_findings(vec![a, b]);
        assert_eq!(
            merged.len(),
            1,
            "overlapping same-category findings collapse"
        );
        let f = &merged[0];
        assert_eq!(f.severity, Severity::Warning, "highest severity kept");
        assert_eq!(
            f.origin,
            Origin::Eslint,
            "representative is the higher-severity finding"
        );
        assert!(
            f.also_from.contains(&Origin::Ruff),
            "other origin unioned in"
        );
        assert!(
            !f.also_from.contains(&f.origin),
            "also_from never holds the primary origin"
        );
    }

    #[test]
    fn test_non_overlapping_same_category_not_merged() {
        let a = finding(
            "a.py",
            Category::DocStyle,
            Range::new(0, 10, 1, 1),
            Origin::Ruff,
            Severity::Info,
        );
        let b = finding(
            "a.py",
            Category::DocStyle,
            Range::new(10, 20, 2, 2),
            Origin::Ruff,
            Severity::Info,
        );
        // Half-open: [0,10) and [10,20) touch but do not overlap.
        assert_eq!(dedup_findings(vec![a, b]).len(), 2);
    }

    #[test]
    fn test_different_category_overlap_not_merged() {
        let a = finding(
            "a.py",
            Category::DocStyle,
            Range::new(0, 10, 1, 1),
            Origin::Ruff,
            Severity::Info,
        );
        let b = finding(
            "a.py",
            Category::CommentedCode,
            Range::new(0, 10, 1, 1),
            Origin::Ruff,
            Severity::Warning,
        );
        assert_eq!(
            dedup_findings(vec![a, b]).len(),
            2,
            "dedup only merges same category"
        );
    }

    #[test]
    fn test_three_way_transitive_overlap_collapses() {
        // [0,6) ∩ [5,11) ∩ [10,16): a–b and b–c overlap, so all three merge.
        let a = finding(
            "a.py",
            Category::DocStyle,
            Range::new(0, 6, 1, 1),
            Origin::Ruff,
            Severity::Info,
        );
        let b = finding(
            "a.py",
            Category::DocStyle,
            Range::new(5, 11, 1, 1),
            Origin::Eslint,
            Severity::Warning,
        );
        let c = finding(
            "a.py",
            Category::DocStyle,
            Range::new(10, 16, 1, 1),
            Origin::Native,
            Severity::Info,
        );
        let merged = dedup_findings(vec![c, a, b]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].severity, Severity::Warning);
        assert!(merged[0].also_from.contains(&Origin::Ruff));
        assert!(merged[0].also_from.contains(&Origin::Native));
    }

    #[test]
    fn test_output_is_canonically_ordered_and_deterministic() {
        let mk = |file: &str, line: u32, byte: u32| {
            finding(
                file,
                Category::DocStyle,
                Range::new(byte, byte + 1, line, line),
                Origin::Ruff,
                Severity::Info,
            )
        };
        let scrambled = vec![
            mk("b.py", 1, 0),
            mk("a.py", 2, 5),
            mk("a.py", 1, 9),
            mk("a.py", 1, 3),
        ];
        let out = dedup_findings(scrambled.clone());
        let keys: Vec<_> = out
            .iter()
            .map(|f| (f.file.clone(), f.range.start_line, f.range.start_byte))
            .collect();
        assert_eq!(
            keys,
            vec![
                ("a.py".to_owned(), 1, 3),
                ("a.py".to_owned(), 1, 9),
                ("a.py".to_owned(), 2, 5),
                ("b.py".to_owned(), 1, 0),
            ]
        );
        // Determinism: a second run on the same (reshuffled) input is identical.
        let mut reshuffled = scrambled;
        reshuffled.reverse();
        assert_eq!(dedup_findings(reshuffled), out);
    }
}
