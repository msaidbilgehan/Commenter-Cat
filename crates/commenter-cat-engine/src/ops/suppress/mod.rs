//! Filter-up suppression (Idea §5; task 7.4).
//!
//! Suppression is applied at the normalization layer — we own the finding
//! ranges, so we own the filter (one syntax, four tools). Suppressed findings
//! are **flagged, not dropped** (`suppressed_by`), kept in the index, excluded
//! from default views, and revealed by `--show-suppressed`. Two capabilities
//! fall out free: an audit trail and **unused-directive detection** (a
//! `commenter-cat:disable` that no longer suppresses anything is reported).
//!
//! Inline directives and the committed baseline (task 7.5) are two inputs to
//! **one** suppression pass.

pub mod directives;
pub mod export;

use commenter_cat_core::comment::Comment;
use commenter_cat_core::finding::Finding;

use crate::ops::baseline::Baseline;
use directives::{Directive, DirectiveKind};

/// One suppression decision: which finding, suppressed by which directive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuppressionDecision {
    /// Index into the findings slice.
    pub finding_index: usize,
    /// The directive that suppressed it (`describe()` form), for `suppressed_by`.
    pub suppressed_by: String,
}

/// The result of one suppression pass.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SuppressionOutcome {
    /// Findings suppressed (flagged, not dropped) and by what.
    pub decisions: Vec<SuppressionDecision>,
    /// Directives that suppressed nothing (reported, like ESLint's
    /// `--report-unused-disable-directives`).
    pub unused_directives: Vec<Directive>,
}

/// Runs the suppression pass over `findings` given inline `directives`.
#[must_use]
pub fn suppress(findings: &[Finding], directives: &[Directive]) -> SuppressionOutcome {
    let mut decisions = Vec::new();
    let mut used = vec![false; directives.len()];

    for (finding_index, finding) in findings.iter().enumerate() {
        if let Some(directive_index) = suppressing_directive(finding, directives) {
            used[directive_index] = true;
            decisions.push(SuppressionDecision {
                finding_index,
                suppressed_by: directives[directive_index].describe(),
            });
        }
    }

    let unused_directives = directives
        .iter()
        .zip(&used)
        .filter(|(_, was_used)| !**was_used)
        .map(|(directive, _)| directive.clone())
        .collect();

    SuppressionOutcome {
        decisions,
        unused_directives,
    }
}

/// A finding suppressed within a `CheckResult`'s comments — located by index and
/// annotated with what suppressed it (Idea §5: flagged, not dropped).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuppressedFinding {
    /// Index into `CheckResult::comments`.
    pub comment_index: usize,
    /// Index into that comment's `findings`.
    pub finding_index: usize,
    /// What suppressed it — a directive's `describe()` form, or `"baseline"`.
    pub suppressed_by: String,
}

/// Runs the unified suppression pass over fused `comments` (Idea §5): inline
/// `commenter-cat:*` directives (parsed from the comments themselves) and the committed
/// Tier-2 `baseline` are the two inputs to **one** pass. Returns the suppressed
/// findings, located + annotated — never dropped, so the caller keeps them in the
/// index and only excludes them from default views.
#[must_use]
pub fn apply(comments: &[Comment], baseline: &Baseline) -> Vec<SuppressedFinding> {
    let directives: Vec<Directive> = comments
        .iter()
        .filter_map(|comment| directives::parse(&comment.raw_text, comment.range.start_line))
        .collect();

    let mut suppressed = Vec::new();
    for (comment_index, comment) in comments.iter().enumerate() {
        // Inline directives apply by line/scope across the whole file, so the
        // global directive set is matched against each comment's findings.
        let outcome = suppress(&comment.findings, &directives);
        for decision in &outcome.decisions {
            suppressed.push(SuppressedFinding {
                comment_index,
                finding_index: decision.finding_index,
                suppressed_by: decision.suppressed_by.clone(),
            });
        }
        // The committed baseline (Tier-2) covers findings no directive caught.
        let symbol = comment.bound_symbol.as_ref().map(|s| s.as_str());
        let fingerprint = comment.cosmetic_fingerprint.as_deref().unwrap_or_default();
        for (finding_index, finding) in comment.findings.iter().enumerate() {
            let by_directive = outcome
                .decisions
                .iter()
                .any(|decision| decision.finding_index == finding_index);
            if by_directive {
                continue;
            }
            if baseline.contains(symbol, fingerprint, &finding.provider_rule_id)
                || baseline.contains(symbol, fingerprint, &finding.canonical_rule_id)
            {
                suppressed.push(SuppressedFinding {
                    comment_index,
                    finding_index,
                    suppressed_by: "baseline".to_owned(),
                });
            }
        }
    }
    suppressed
}

/// The index of the first directive that suppresses `finding`, if any.
fn suppressing_directive(finding: &Finding, directives: &[Directive]) -> Option<usize> {
    let line = finding.range.start_line;
    directives
        .iter()
        .enumerate()
        .find_map(|(index, directive)| {
            (target_matches(finding, directive) && scope_covers(directive, line, directives))
                .then_some(index)
        })
}

/// Whether a directive's target list matches a finding (empty = all). Accepts
/// any level: provider_rule_id, bare canonical id, category, or origin (Idea §5).
fn target_matches(finding: &Finding, directive: &Directive) -> bool {
    directive.targets.is_empty()
        || directive.targets.iter().any(|target| {
            target == &finding.provider_rule_id
                || target == &finding.canonical_rule_id
                || target == finding.category.as_str()
                || target == finding.origin.as_str()
        })
}

/// Whether a directive's *scope* covers `finding_line` (Idea §5).
fn scope_covers(directive: &Directive, finding_line: u32, all: &[Directive]) -> bool {
    match directive.kind {
        DirectiveKind::DisableFile => true,
        DirectiveKind::DisableLine => directive.line == finding_line,
        DirectiveKind::DisableNextLine => directive.line + 1 == finding_line,
        DirectiveKind::Disable => {
            directive.line <= finding_line && !reenabled_between(directive, finding_line, all)
        }
        DirectiveKind::Enable => false,
    }
}

/// Whether a matching `commenter-cat:enable` closes `disable`'s region before `finding_line`.
fn reenabled_between(disable: &Directive, finding_line: u32, all: &[Directive]) -> bool {
    all.iter().any(|enable| {
        enable.kind == DirectiveKind::Enable
            && enable.line > disable.line
            && enable.line <= finding_line
            // An `enable` with no targets closes everything; otherwise it must
            // overlap the disable's targets.
            && (enable.targets.is_empty()
                || disable.targets.is_empty()
                || enable.targets.iter().any(|t| disable.targets.contains(t)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use commenter_cat_core::finding::{Category, FindingTarget, Fix, Origin, Range};
    use commenter_cat_core::severity::Severity;
    use commenter_cat_core::symbol::CommentId;

    fn finding(rule: &str, category: Category, line: u32) -> Finding {
        Finding {
            file: "a.py".to_owned(),
            target: FindingTarget::Comment(CommentId::new("c")),
            range: Range::new(0, 1, line, line),
            origin: Origin::Ruff,
            provider_rule_id: format!("ruff:{rule}"),
            canonical_rule_id: rule.to_owned(),
            category,
            severity: Severity::Warning,
            severity_native: None,
            message: "m".to_owned(),
            fix: Fix::None,
            url: None,
            also_from: Default::default(),
        }
    }

    #[test]
    fn test_disable_line_and_next_line_scopes() {
        let findings = [finding("D417", Category::DocDrift, 5)];
        let on_line = directives::parse("# commenter-cat:disable-line=D417", 5).unwrap();
        assert_eq!(suppress(&findings, &[on_line]).decisions.len(), 1);

        let next_line = directives::parse("# commenter-cat:disable-next-line=D417", 4).unwrap();
        assert_eq!(suppress(&findings, &[next_line]).decisions.len(), 1);

        let wrong_line = directives::parse("# commenter-cat:disable-line=D417", 9).unwrap();
        assert!(suppress(&findings, &[wrong_line]).decisions.is_empty());
    }

    #[test]
    fn test_category_and_origin_targets() {
        let findings = [finding("D417", Category::DocDrift, 5)];
        let by_category = directives::parse("# commenter-cat:disable-line=doc_drift", 5).unwrap();
        assert_eq!(suppress(&findings, &[by_category]).decisions.len(), 1);
        let by_origin = directives::parse("# commenter-cat:disable-line=ruff", 5).unwrap();
        assert_eq!(suppress(&findings, &[by_origin]).decisions.len(), 1);
        let by_other = directives::parse("# commenter-cat:disable-line=eslint", 5).unwrap();
        assert!(suppress(&findings, &[by_other]).decisions.is_empty());
    }

    #[test]
    fn test_disable_enable_region() {
        let findings = [
            finding("D417", Category::DocDrift, 3), // inside region
            finding("D417", Category::DocDrift, 9), // after enable
        ];
        let directives = [
            directives::parse("# commenter-cat:disable=D417", 1).unwrap(),
            directives::parse("# commenter-cat:enable=D417", 6).unwrap(),
        ];
        let outcome = suppress(&findings, &directives);
        assert_eq!(
            outcome.decisions,
            vec![SuppressionDecision {
                finding_index: 0,
                suppressed_by: "commenter-cat:disable=D417".to_owned()
            }]
        );
    }

    #[test]
    fn test_disable_file_and_unused_reported() {
        let findings = [finding("D417", Category::DocDrift, 5)];
        let directives = [
            directives::parse("# commenter-cat:disable-file=D417", 1).unwrap(),
            directives::parse("# commenter-cat:disable-line=D100", 2).unwrap(), // matches nothing
        ];
        let outcome = suppress(&findings, &directives);
        assert_eq!(outcome.decisions.len(), 1);
        assert_eq!(outcome.unused_directives.len(), 1);
        assert_eq!(
            outcome.unused_directives[0].describe(),
            "commenter-cat:disable-line=D100"
        );
    }
}
