//! Markdown report and CSV renderers (Idea §8).
//!
//! Two flat, finding-oriented views: a Markdown table for PR/issue bodies and an
//! RFC 4180 CSV for spreadsheets. Both derive from the same flattened
//! `(comment, finding)` stream.

use commenter_cat_core::comment::Comment;

use super::findings_with_context;

/// Renders a Markdown report (a single findings table).
#[must_use]
pub fn render_markdown(comments: &[Comment]) -> String {
    let pairs = findings_with_context(comments);
    let mut out = String::from("# Comment findings\n\n");
    if pairs.is_empty() {
        out.push_str("_No findings._\n");
        return out;
    }
    out.push_str(&format!("{} finding(s).\n\n", pairs.len()));
    out.push_str("| File | Line | Severity | Rule | Category | Message |\n");
    out.push_str("|------|------|----------|------|----------|---------|\n");
    for (_, finding) in &pairs {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            finding.file,
            finding.range.start_line,
            finding.severity.as_str(),
            finding.provider_rule_id,
            finding.category.as_str(),
            escape_markdown(&finding.message),
        ));
    }
    out
}

/// Renders an RFC 4180 CSV (header + one row per finding).
#[must_use]
pub fn render_csv(comments: &[Comment]) -> String {
    let mut out = String::from("file,line,severity,origin,rule,category,message\n");
    for (_, finding) in &findings_with_context(comments) {
        let row = [
            finding.file.as_str(),
            &finding.range.start_line.to_string(),
            finding.severity.as_str(),
            finding.origin.as_str(),
            finding.provider_rule_id.as_str(),
            finding.category.as_str(),
            finding.message.as_str(),
        ];
        let line: Vec<String> = row.iter().map(|field| escape_csv(field)).collect();
        out.push_str(&line.join(","));
        out.push('\n');
    }
    out
}

/// Escapes a pipe so it cannot break the Markdown table.
fn escape_markdown(field: &str) -> String {
    field.replace('|', "\\|").replace('\n', " ")
}

/// Escapes a CSV field per RFC 4180: quote if it contains a comma, quote, or
/// newline; double any embedded quotes.
fn escape_csv(field: &str) -> String {
    if field.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::super::testkit::*;
    use super::*;
    use commenter_cat_core::finding::Category;
    use commenter_cat_core::severity::Severity;

    #[test]
    fn test_markdown_table_has_header_and_row() {
        let comments = vec![comment_with(vec![finding(
            "D417",
            Severity::Error,
            Category::DocDrift,
        )])];
        let md = render_markdown(&comments);
        assert!(md.contains("| File | Line | Severity | Rule | Category | Message |"));
        assert!(md.contains("| pkg/app.py | 1 | error | ruff:D417 | doc_drift |"));
    }

    #[test]
    fn test_csv_quotes_fields_with_commas() {
        let mut f = finding("D417", Severity::Error, Category::DocDrift);
        f.message = "drift, see \"docs\"".to_owned();
        let csv = render_csv(&[comment_with(vec![f])]);
        // The message with a comma + quotes is RFC-4180 escaped.
        assert!(csv.contains("\"drift, see \"\"docs\"\"\""));
        assert!(csv.starts_with("file,line,severity,origin,rule,category,message\n"));
    }

    #[test]
    fn test_empty_markdown() {
        assert!(render_markdown(&[comment_with(vec![])]).contains("_No findings._"));
    }
}
