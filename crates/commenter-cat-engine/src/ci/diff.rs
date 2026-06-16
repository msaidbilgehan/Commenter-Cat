//! The CI verdict (Idea §7).
//!
//! The build fails on findings at/above `fail_on` that are **not** in the
//! committed baseline. Crucially, a **PARTIAL** provider (findings unavailable —
//! a crash/timeout, not zero findings) downgrades the verdict to **degraded**
//! rather than passing silently: an absent signal is never a green check.

use commenter_cat_core::comment::Comment;
use commenter_cat_core::finding::Finding;
use commenter_cat_core::severity::Severity;

use crate::ops::baseline::Baseline;
use crate::provider::RunState;

/// The CI verdict (Idea §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// No new findings at/above the gate.
    Pass,
    /// New, non-baselined findings at/above the gate.
    Fail,
    /// A provider was PARTIAL — the diff cannot be trusted (never a silent pass).
    Degraded,
}

impl Verdict {
    /// The CI process exit code (`0` pass, `1` fail, `2` degraded).
    #[must_use]
    pub const fn exit_code(self) -> i32 {
        match self {
            Verdict::Pass => 0,
            Verdict::Fail => 1,
            Verdict::Degraded => 2,
        }
    }
}

/// Findings not matched by the committed baseline (Tier-2: bound symbol +
/// cosmetic fingerprint + rule) — the "new since baseline" set (Idea §5, §7).
#[must_use]
pub fn new_findings<'a>(comments: &'a [Comment], baseline: &Baseline) -> Vec<&'a Finding> {
    let mut new = Vec::new();
    for comment in comments {
        let symbol = comment
            .bound_symbol
            .as_ref()
            .map(commenter_cat_core::symbol::BoundSymbol::as_str);
        let fingerprint = comment.cosmetic_fingerprint.as_deref().unwrap_or_default();
        for finding in &comment.findings {
            let baselined = baseline.contains(symbol, fingerprint, &finding.provider_rule_id)
                || baseline.contains(symbol, fingerprint, &finding.canonical_rule_id);
            if !baselined {
                new.push(finding);
            }
        }
    }
    new
}

/// Whether any provider produced a PARTIAL run (findings unavailable, Idea §5).
#[must_use]
pub fn any_partial(run_states: &[(String, RunState)]) -> bool {
    run_states
        .iter()
        .any(|(_, state)| *state == RunState::Partial)
}

/// The verdict from the new findings, the gate, and whether any provider was
/// PARTIAL (Idea §7).
#[must_use]
pub fn verdict(new_findings: &[&Finding], fail_on: Severity, any_partial: bool) -> Verdict {
    // A missing provider signal poisons the whole diff — degrade, don't pass.
    if any_partial {
        return Verdict::Degraded;
    }
    if new_findings
        .iter()
        .any(|finding| finding.severity.fails_ci(fail_on))
    {
        Verdict::Fail
    } else {
        Verdict::Pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::baseline;
    use commenter_cat_core::finding::{Category, FindingTarget, Fix, Origin, Range};
    use commenter_cat_core::kind::CommentKind;
    use commenter_cat_core::lang::Language;
    use commenter_cat_core::symbol::{BoundSymbol, CommentId};

    fn comment_with(symbol: &str, fingerprint: &str, findings: Vec<Finding>) -> Comment {
        let mut c = Comment::new(
            "a.py",
            "h",
            Language::Python,
            CommentKind::Line,
            Range::new(0, 6, 1, 1),
            "# c",
        );
        c.bound_symbol = Some(BoundSymbol::new(symbol));
        c.cosmetic_fingerprint = Some(fingerprint.to_owned());
        c.findings = findings;
        c
    }

    fn finding(rule: &str, severity: Severity) -> Finding {
        Finding {
            file: "a.py".to_owned(),
            target: FindingTarget::Comment(CommentId::new("c")),
            range: Range::new(0, 6, 1, 1),
            origin: Origin::Ruff,
            provider_rule_id: format!("ruff:{rule}"),
            canonical_rule_id: rule.to_owned(),
            category: Category::DocDrift,
            severity,
            severity_native: None,
            message: "m".to_owned(),
            fix: Fix::None,
            url: None,
            also_from: Default::default(),
        }
    }

    #[test]
    fn test_baselined_findings_are_not_new() {
        let comments = [comment_with(
            "app.f",
            "fp1",
            vec![finding("D417", Severity::Error)],
        )];
        // The baseline already covers app.f/fp1/ruff:D417.
        let base = baseline::accept(&[(
            Some("app.f".to_owned()),
            "fp1".to_owned(),
            "ruff:D417".to_owned(),
        )]);
        assert!(new_findings(&comments, &base).is_empty());

        // An empty baseline → the finding is new.
        assert_eq!(
            new_findings(&comments, &baseline::Baseline::default()).len(),
            1
        );
    }

    #[test]
    fn test_partial_provider_degrades_not_passes() {
        // No failing findings at all, but a PARTIAL provider → degraded.
        let partial = [("ruff".to_owned(), RunState::Partial)];
        assert!(any_partial(&partial));
        assert_eq!(verdict(&[], Severity::Error, true), Verdict::Degraded);
        assert_eq!(Verdict::Degraded.exit_code(), 2);
    }

    #[test]
    fn test_fail_and_pass() {
        let failing = finding("D417", Severity::Error);
        let warning = finding("D100", Severity::Warning);
        assert_eq!(verdict(&[&failing], Severity::Error, false), Verdict::Fail);
        assert_eq!(verdict(&[&warning], Severity::Error, false), Verdict::Pass);
        assert_eq!(verdict(&[], Severity::Error, false), Verdict::Pass);
    }
}
