//! Output renderers (Idea §8) — structured-first, JSONL canonical.
//!
//! Every format is a *renderer* over the one check result: [`jsonl`] is the
//! canonical `schema_version`-tagged stream (round-trips), [`terminal`] is the
//! grouped human view, [`sarif`] emits GitHub code-scanning, and
//! [`markdown_csv`] the report/spreadsheet forms. [`ci_exit_code`] turns the
//! result into a build verdict: fail at/above `fail_on`, and always on
//! `DO_NOT_MERGE` (Idea §8).

pub mod jsonl;
pub mod markdown_csv;
pub mod sarif;
pub mod terminal;

use cf_core::comment::Comment;
use cf_core::config::OutputFormat;
use cf_core::error::CfResult;
use cf_core::finding::Finding;
use cf_core::severity::Severity;

/// Clean run — no build-failing findings.
pub const EXIT_OK: i32 = 0;
/// At least one finding at/above `fail_on`, or an always-fail marker.
pub const EXIT_FINDINGS: i32 = 1;

/// Markers that fail CI unconditionally, regardless of `fail_on` (Idea §8/§9 —
/// `DO_NOT_MERGE` is a hard gate, not a severity threshold).
pub const ALWAYS_FAIL_MARKERS: [&str; 1] = ["DO_NOT_MERGE"];

/// Renders the check result (`comments` with fused findings) in `format`.
///
/// # Errors
/// Returns [`cf_core::CfError`] if a structured format fails to serialize.
pub fn render(comments: &[Comment], format: OutputFormat) -> CfResult<String> {
    match format {
        OutputFormat::Jsonl => jsonl::render(comments),
        OutputFormat::Terminal => Ok(terminal::render(comments)),
        OutputFormat::Sarif => sarif::render(comments),
        OutputFormat::Markdown => Ok(markdown_csv::render_markdown(comments)),
        OutputFormat::Csv => Ok(markdown_csv::render_csv(comments)),
    }
}

/// The CI verdict: [`EXIT_FINDINGS`] if any finding is at/above `fail_on`, or any
/// always-fail marker is present; otherwise [`EXIT_OK`] (Idea §8).
#[must_use]
pub fn ci_exit_code(comments: &[Comment], fail_on: Severity) -> i32 {
    let fails = comments.iter().any(|comment| {
        comment
            .findings
            .iter()
            .any(|finding| finding.severity.fails_ci(fail_on))
            || comment
                .markers
                .iter()
                .any(|marker| ALWAYS_FAIL_MARKERS.contains(&marker.as_str()))
    });
    if fails {
        EXIT_FINDINGS
    } else {
        EXIT_OK
    }
}

/// A flattened `(comment, finding)` view for finding-oriented renderers (SARIF,
/// CSV, markdown). Order follows the comments' canonical order.
#[must_use]
pub(crate) fn findings_with_context(comments: &[Comment]) -> Vec<(&Comment, &Finding)> {
    comments
        .iter()
        .flat_map(|comment| {
            comment
                .findings
                .iter()
                .map(move |finding| (comment, finding))
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod testkit {
    //! Shared fixtures for the renderer tests.
    use cf_core::comment::Comment;
    use cf_core::finding::{Category, Finding, FindingTarget, Fix, Origin, Range};
    use cf_core::kind::CommentKind;
    use cf_core::lang::Language;
    use cf_core::severity::Severity;
    use cf_core::symbol::CommentId;

    pub(crate) fn finding(rule: &str, severity: Severity, category: Category) -> Finding {
        Finding {
            file: "pkg/app.py".to_owned(),
            target: FindingTarget::Comment(CommentId::new("c")),
            range: Range::new(0, 6, 1, 1),
            origin: Origin::Ruff,
            provider_rule_id: format!("ruff:{rule}"),
            canonical_rule_id: rule.to_owned(),
            category,
            severity,
            severity_native: None,
            message: format!("{rule} on this comment"),
            fix: Fix::ProviderAutofix,
            url: Some("https://docs.example/".to_owned() + rule),
            also_from: Default::default(),
        }
    }

    pub(crate) fn comment_with(findings: Vec<Finding>) -> Comment {
        let mut c = Comment::new(
            "pkg/app.py",
            "hash1",
            Language::Python,
            CommentKind::Line,
            Range::new(0, 6, 1, 1),
            "# todo",
        );
        c.findings = findings;
        c
    }
}

#[cfg(test)]
mod tests {
    use super::testkit::*;
    use super::*;
    use cf_core::finding::Category;

    #[test]
    fn test_exit_code_fails_at_or_above_fail_on() {
        let comments = [comment_with(vec![finding(
            "D417",
            Severity::Error,
            Category::DocDrift,
        )])];
        assert_eq!(ci_exit_code(&comments, Severity::Error), EXIT_FINDINGS);
        // A warning does not fail an `error` threshold.
        let warn = [comment_with(vec![finding(
            "D100",
            Severity::Warning,
            Category::DocMissing,
        )])];
        assert_eq!(ci_exit_code(&warn, Severity::Error), EXIT_OK);
    }

    #[test]
    fn test_do_not_merge_marker_always_fails() {
        let mut comment = comment_with(vec![]);
        comment.markers = vec!["DO_NOT_MERGE".to_owned()];
        // No findings at all, yet the marker is a hard gate.
        assert_eq!(ci_exit_code(&[comment], Severity::Critical), EXIT_FINDINGS);
    }

    #[test]
    fn test_clean_result_exits_ok() {
        assert_eq!(
            ci_exit_code(&[comment_with(vec![])], Severity::Error),
            EXIT_OK
        );
    }
}
