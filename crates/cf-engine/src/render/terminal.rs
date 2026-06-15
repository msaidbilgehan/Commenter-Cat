//! The grouped terminal view (Idea §8).
//!
//! The default human format: findings grouped by file, each line carrying
//! severity, rule, location, and message, with a closing summary. No color codes
//! (so the output is pipe- and CI-log-friendly); structure carries the meaning.

use std::collections::BTreeMap;

use cf_core::comment::Comment;
use cf_core::finding::Finding;

use super::findings_with_context;

/// Renders the check result grouped by file.
#[must_use]
pub fn render(comments: &[Comment]) -> String {
    let pairs = findings_with_context(comments);
    if pairs.is_empty() {
        return "No comment findings.\n".to_owned();
    }

    let mut by_file: BTreeMap<&str, Vec<&Finding>> = BTreeMap::new();
    for (_, finding) in &pairs {
        by_file
            .entry(finding.file.as_str())
            .or_default()
            .push(finding);
    }

    let mut out = String::new();
    for (file, findings) in &by_file {
        out.push_str(file);
        out.push('\n');
        for finding in findings {
            out.push_str(&format!(
                "  {:<8} {:<24} line {:<5} {}\n",
                finding.severity.as_str(),
                finding.provider_rule_id,
                finding.range.start_line,
                finding.message,
            ));
        }
        out.push('\n');
    }

    out.push_str(&format!(
        "{} finding(s) across {} file(s).\n",
        pairs.len(),
        by_file.len()
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::super::testkit::*;
    use super::*;
    use cf_core::finding::Category;
    use cf_core::severity::Severity;

    #[test]
    fn test_groups_by_file_with_summary() {
        let comments = vec![comment_with(vec![finding(
            "D417",
            Severity::Error,
            Category::DocDrift,
        )])];
        let out = render(&comments);
        assert!(out.contains("pkg/app.py"));
        assert!(out.contains("error"));
        assert!(out.contains("ruff:D417"));
        assert!(out.contains("1 finding(s) across 1 file(s)."));
    }

    #[test]
    fn test_empty_result_message() {
        assert_eq!(render(&[comment_with(vec![])]), "No comment findings.\n");
    }
}
