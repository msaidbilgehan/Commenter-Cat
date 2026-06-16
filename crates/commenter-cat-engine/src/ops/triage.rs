//! Native finding triage (Idea §3, §9; task 7.1).
//!
//! Findings the providers can't produce: blame-skew **rot candidates** (a
//! token-free shortlist the agent judges) and **cross-language marker triage**
//! (marker × severity × blame-age) — the ranked worklist. Both are `origin =
//! native`, agent-judged (`fix = agent_only`).

use std::collections::BTreeMap;

use commenter_cat_core::comment::Comment;
use commenter_cat_core::finding::{Category, Finding, FindingTarget, Fix, Origin, Range};
use commenter_cat_core::severity::Severity;
use commenter_cat_core::symbol::CommentId;

/// Native marker findings for a comment — one per marker tag (Idea §9). Severity
/// comes from `[markers].severity`, else the `marker_stale` anchor. Each finding
/// is anchored to the marker's own span, so distinct markers on one (coalesced)
/// comment get distinct ranges and never false-merge under same-category dedup.
#[must_use]
pub fn marker_findings(
    comment: &Comment,
    marker_severities: &BTreeMap<String, Severity>,
) -> Vec<Finding> {
    comment
        .markers
        .iter()
        .map(|marker| {
            let severity = marker_severities
                .get(marker)
                .copied()
                .unwrap_or_else(|| Category::MarkerStale.canonical_severity());
            native_finding(
                comment,
                marker_range(comment, marker),
                Category::MarkerStale,
                format!("marker:{marker}"),
                severity,
                format!("{marker} marker"),
            )
        })
        .collect()
}

/// A native rot-candidate finding for a flagged comment, if any (Idea §3, §9).
#[must_use]
pub fn rot_finding(comment: &Comment) -> Option<Finding> {
    comment.is_rot_candidate.then(|| {
        native_finding(
            comment,
            comment.range,
            Category::RotCandidate,
            "rot_candidate".to_owned(),
            Category::RotCandidate.canonical_severity(),
            "comment blame predates its bound code — may be stale".to_owned(),
        )
    })
}

/// The span of `marker`'s first occurrence within `comment`, or the whole
/// comment range if it cannot be located (e.g. a regex-tagged custom marker).
fn marker_range(comment: &Comment, marker: &str) -> Range {
    let Some(offset) = comment.raw_text.find(marker) else {
        return comment.range;
    };
    let (Ok(offset), Ok(len)) = (u32::try_from(offset), u32::try_from(marker.len())) else {
        return comment.range;
    };
    let start = comment.range.start_byte + offset;
    Range::new(
        start,
        start + len,
        comment.range.start_line,
        comment.range.end_line,
    )
}

/// Builds a native finding anchored to `range` within a comment.
fn native_finding(
    comment: &Comment,
    range: Range,
    category: Category,
    rule: String,
    severity: Severity,
    message: String,
) -> Finding {
    Finding {
        file: comment.path.clone(),
        target: FindingTarget::Comment(CommentId::new(&comment.content_hash)),
        range,
        origin: Origin::Native,
        provider_rule_id: format!("native:{rule}"),
        canonical_rule_id: rule,
        category,
        severity,
        severity_native: None,
        message,
        // Native drift is the agent's to judge (Idea §1, §9).
        fix: Fix::AgentOnly,
        url: None,
        also_from: std::collections::BTreeSet::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use commenter_cat_core::finding::Range;
    use commenter_cat_core::kind::CommentKind;
    use commenter_cat_core::lang::Language;

    fn comment() -> Comment {
        let mut c = Comment::new(
            "a.py",
            "hash",
            Language::Python,
            CommentKind::Line,
            Range::new(0, 20, 1, 1),
            "# TODO: fix; DO_NOT_MERGE",
        );
        c.markers = vec!["TODO".to_owned(), "DO_NOT_MERGE".to_owned()];
        c
    }

    #[test]
    fn test_marker_findings_use_config_severity() {
        let severities: BTreeMap<String, Severity> =
            [("DO_NOT_MERGE".to_owned(), Severity::Critical)]
                .into_iter()
                .collect();
        let findings = marker_findings(&comment(), &severities);
        assert_eq!(findings.len(), 2);
        let dnm = findings
            .iter()
            .find(|f| f.message.contains("DO_NOT_MERGE"))
            .unwrap();
        assert_eq!(dnm.severity, Severity::Critical);
        let todo = findings
            .iter()
            .find(|f| f.message.contains("TODO"))
            .unwrap();
        assert_eq!(
            todo.severity,
            Severity::Warning,
            "default marker_stale anchor"
        );
        assert!(findings.iter().all(|f| f.origin == Origin::Native));
    }

    #[test]
    fn test_rot_finding_only_when_flagged() {
        let mut c = comment();
        assert!(rot_finding(&c).is_none());
        c.is_rot_candidate = true;
        let finding = rot_finding(&c).unwrap();
        assert_eq!(finding.category, Category::RotCandidate);
        assert_eq!(finding.origin, Origin::Native);
    }
}
