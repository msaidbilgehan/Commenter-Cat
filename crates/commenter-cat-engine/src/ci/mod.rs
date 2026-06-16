//! CI integration (Idea §7) — shared truth is CI.
//!
//! Restore the two cache layers (so a Commenter-Cat upgrade reuses provider results), diff
//! the unified findings against the committed baseline, downgrade to a degraded
//! verdict on a PARTIAL provider rather than passing silently, and publish the
//! report artifacts (SARIF + markdown summary + JSONL). Byte-identical reports
//! follow from the canonical finding ordering (Phase 3).

pub mod cache_artifacts;
pub mod diff;

use commenter_cat_core::comment::Comment;
use commenter_cat_core::error::CommenterCatResult;
use commenter_cat_core::severity::Severity;

use crate::ops::baseline::Baseline;
use crate::provider::RunState;
use crate::render;

pub use cache_artifacts::{
    index_cache_key, inputs_cache_key, plan_restore, ProviderFingerprint, RestorePlan,
};
pub use diff::{any_partial, new_findings, verdict, Verdict};

/// A CI run's outcome plus the artifacts to publish (Idea §7).
#[derive(Debug)]
pub struct CiReport {
    /// Pass / Fail / Degraded.
    pub verdict: Verdict,
    /// How the caches were restored (the Commenter-Cat-upgrade fast path is observable here).
    pub restore_plan: RestorePlan,
    /// The count of new, non-baselined findings.
    pub new_findings: usize,
    /// SARIF 2.1.0 (GitHub code-scanning upload).
    pub sarif: String,
    /// Markdown summary (the PR/job summary).
    pub markdown: String,
    /// Canonical JSONL (the durable artifact).
    pub jsonl: String,
}

/// Produces the CI report from a check result, the committed baseline, the gate,
/// and the two cache hits (Idea §7).
///
/// # Errors
/// Returns [`commenter_cat_core::CommenterCatError`] if a structured artifact fails to render.
pub fn run(
    comments: &[Comment],
    run_states: &[(String, RunState)],
    baseline: &Baseline,
    fail_on: Severity,
    inputs_hit: bool,
    index_hit: bool,
) -> CommenterCatResult<CiReport> {
    let restore_plan = plan_restore(inputs_hit, index_hit);
    let new = new_findings(comments, baseline);
    let verdict = verdict(&new, fail_on, any_partial(run_states));

    Ok(CiReport {
        verdict,
        restore_plan,
        new_findings: new.len(),
        sarif: render::sarif::render(comments)?,
        markdown: render::markdown_csv::render_markdown(comments),
        jsonl: render::jsonl::render(comments)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::baseline::Baseline;
    use commenter_cat_core::finding::{Category, Finding, FindingTarget, Fix, Origin, Range};
    use commenter_cat_core::kind::CommentKind;
    use commenter_cat_core::lang::Language;
    use commenter_cat_core::symbol::CommentId;

    fn comment(findings: Vec<Finding>) -> Comment {
        let mut c = Comment::new(
            "a.py",
            "h",
            Language::Python,
            CommentKind::Line,
            Range::new(0, 6, 1, 1),
            "# c",
        );
        c.findings = findings;
        c
    }

    fn finding(severity: Severity) -> Finding {
        Finding {
            file: "a.py".to_owned(),
            target: FindingTarget::Comment(CommentId::new("c")),
            range: Range::new(0, 6, 1, 1),
            origin: Origin::Ruff,
            provider_rule_id: "ruff:D417".to_owned(),
            canonical_rule_id: "D417".to_owned(),
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
    fn test_commenter_cat_upgrade_reuses_inputs_and_rederives_index() {
        // inputs hit, index miss (the Commenter-Cat-upgrade case) → re-derive, no cold scan.
        let report = run(
            &[comment(vec![])],
            &[("ruff".to_owned(), RunState::Success)],
            &Baseline::default(),
            Severity::Error,
            true,
            false,
        )
        .unwrap();
        assert_eq!(report.restore_plan, RestorePlan::ReDeriveFromInputs);
        assert_eq!(report.verdict, Verdict::Pass);
    }

    #[test]
    fn test_partial_provider_yields_degraded() {
        let report = run(
            &[comment(vec![])],
            &[("ruff".to_owned(), RunState::Partial)],
            &Baseline::default(),
            Severity::Error,
            true,
            true,
        )
        .unwrap();
        assert_eq!(report.verdict, Verdict::Degraded);
        assert_eq!(report.restore_plan, RestorePlan::UseIndex);
    }

    #[test]
    fn test_new_failing_finding_fails_and_publishes_artifacts() {
        let report = run(
            &[comment(vec![finding(Severity::Error)])],
            &[("ruff".to_owned(), RunState::Success)],
            &Baseline::default(),
            Severity::Error,
            false,
            false,
        )
        .unwrap();
        assert_eq!(report.verdict, Verdict::Fail);
        assert_eq!(report.new_findings, 1);
        assert_eq!(report.restore_plan, RestorePlan::ColdScan);
        // All three artifacts are published.
        assert!(report.sarif.contains("\"version\": \"2.1.0\""));
        assert!(report.markdown.contains("Comment findings"));
        assert!(report.jsonl.contains("schema_version"));
    }
}
