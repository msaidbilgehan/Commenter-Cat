//! Finding attachment + per-comment dedup (Idea §4, §5; task 7.1).
//!
//! The normalization layer is where every tool's output and the native facts
//! become **one** record. A finding attaches to the comment it concerns — by
//! **bound symbol** (the finding targets the symbol the comment documents) or by
//! **location** (the finding points at the comment's own bytes). Findings that
//! match no comment (a `doc_missing` on an undocumented symbol) are returned for
//! the caller to surface separately.

use commenter_cat_core::comment::Comment;
use commenter_cat_core::finding::{dedup_findings, Finding, FindingTarget};

/// Attaches each finding to the comment it concerns, returning findings that
/// matched no comment.
#[must_use]
pub fn attach_findings(comments: &mut [Comment], findings: Vec<Finding>) -> Vec<Finding> {
    let mut unattached = Vec::new();
    for finding in findings {
        match comments
            .iter_mut()
            .find(|comment| attaches_to(comment, &finding))
        {
            Some(comment) => comment.findings.push(finding),
            None => unattached.push(finding),
        }
    }
    unattached
}

/// Whether `finding` belongs on `comment` — same file, then by bound symbol or
/// by location (Idea §4).
fn attaches_to(comment: &Comment, finding: &Finding) -> bool {
    if comment.path != finding.file {
        return false;
    }
    if let FindingTarget::Symbol(symbol) = &finding.target {
        if comment.bound_symbol.as_ref() == Some(symbol) {
            return true;
        }
    }
    within_comment_span(comment, finding)
}

/// Whether `finding`'s source location lies **inside** `comment`'s span — the pure
/// location test, independent of bound-symbol attachment.
///
/// A provider finding usually carries a zero-width point (line+column → one byte),
/// so this attaches when that point sits within the comment — including its first
/// byte, where strict `overlaps` alone would miss ruff's `ERA001` on the `#`;
/// `overlaps` still covers wide findings that start before the comment. It is the
/// predicate a **comment-scoped** provider (gitleaks) is filtered by at fusion: a
/// secret counts only when it sits *in a comment* (Idea §5). Because "kept by this
/// predicate" implies "attaches here", such findings never leak out as unattached.
#[must_use]
pub fn within_comment_span(comment: &Comment, finding: &Finding) -> bool {
    comment.path == finding.file
        && (comment.range.contains_byte(finding.range.start_byte)
            || comment.range.overlaps(&finding.range))
}

/// Dedups each comment's findings into canonical order (Idea §5 — same drift seen
/// by two tools collapses to one finding carrying `also_from`).
pub fn dedup_comment_findings(comments: &mut [Comment]) {
    for comment in comments.iter_mut() {
        let findings = std::mem::take(&mut comment.findings);
        comment.findings = dedup_findings(findings);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use commenter_cat_core::finding::{Category, Fix, Origin, Range};
    use commenter_cat_core::kind::CommentKind;
    use commenter_cat_core::lang::Language;
    use commenter_cat_core::severity::Severity;
    use commenter_cat_core::symbol::{BoundSymbol, CommentId};

    fn comment_at(path: &str, range: Range, bound: Option<&str>) -> Comment {
        let mut c = Comment::new(path, "h", Language::Python, CommentKind::Line, range, "# c");
        c.bound_symbol = bound.map(BoundSymbol::new);
        c
    }

    fn finding(path: &str, target: FindingTarget, range: Range, rule: &str) -> Finding {
        Finding {
            file: path.to_owned(),
            target,
            range,
            origin: Origin::Ruff,
            provider_rule_id: format!("ruff:{rule}"),
            canonical_rule_id: rule.to_owned(),
            category: Category::DocDrift,
            severity: Severity::Warning,
            severity_native: None,
            message: "m".to_owned(),
            fix: Fix::None,
            url: None,
            also_from: Default::default(),
        }
    }

    #[test]
    fn test_attach_by_bound_symbol() {
        // The finding targets the symbol; its range need not overlap the comment.
        let mut comments = [comment_at("a.py", Range::new(0, 10, 1, 1), Some("app.f"))];
        let f = finding(
            "a.py",
            FindingTarget::Symbol(BoundSymbol::new("app.f")),
            Range::new(50, 60, 5, 5),
            "D417",
        );
        let unattached = attach_findings(&mut comments, vec![f]);
        assert!(unattached.is_empty());
        assert_eq!(comments[0].findings.len(), 1);
    }

    #[test]
    fn test_attach_by_location() {
        let mut comments = [comment_at("a.py", Range::new(0, 10, 1, 1), None)];
        let f = finding(
            "a.py",
            FindingTarget::Comment(CommentId::new("x")),
            Range::new(2, 8, 1, 1),
            "ERA001",
        );
        assert!(attach_findings(&mut comments, vec![f]).is_empty());
        assert_eq!(comments[0].findings.len(), 1);
    }

    #[test]
    fn test_attach_zero_width_finding_at_comment_start() {
        // ruff's ERA001 points a zero-width range at the comment's first byte
        // (the `#`); strict `overlaps` misses it, point-containment attaches it.
        let mut comments = [comment_at("a.py", Range::new(0, 30, 2, 2), Some("add"))];
        let f = finding(
            "a.py",
            FindingTarget::Symbol(BoundSymbol::new("a.py")), // tool reports file, not "add"
            Range::new(0, 0, 2, 2),
            "ERA001",
        );
        let unattached = attach_findings(&mut comments, vec![f]);
        assert!(
            unattached.is_empty(),
            "a finding on the comment's first byte attaches"
        );
        assert_eq!(comments[0].findings.len(), 1);
    }

    #[test]
    fn test_unmatched_finding_returned() {
        let mut comments = [comment_at("a.py", Range::new(0, 10, 1, 1), Some("app.f"))];
        // Targets a different symbol, no byte overlap → unattached.
        let f = finding(
            "a.py",
            FindingTarget::Symbol(BoundSymbol::new("app.other")),
            Range::new(50, 60, 5, 5),
            "D100",
        );
        let unattached = attach_findings(&mut comments, vec![f]);
        assert_eq!(unattached.len(), 1);
        assert!(comments[0].findings.is_empty());
    }

    #[test]
    fn test_dedup_collapses_duplicate_findings() {
        let mut comments = [comment_at("a.py", Range::new(0, 10, 1, 1), None)];
        let range = Range::new(2, 8, 1, 1);
        let dup = finding(
            "a.py",
            FindingTarget::Comment(CommentId::new("x")),
            range,
            "ERA001",
        );
        assert!(attach_findings(&mut comments, vec![dup.clone(), dup]).is_empty());
        assert_eq!(comments[0].findings.len(), 2, "not yet deduped");
        dedup_comment_findings(&mut comments);
        assert_eq!(comments[0].findings.len(), 1, "deduped to one");
    }
}
