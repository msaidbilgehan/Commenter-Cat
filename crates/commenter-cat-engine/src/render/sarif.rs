//! SARIF 2.1.0 emit (Idea §8) — the GitHub code-scanning format.
//!
//! This is the **emit** side; the ingest side (reading a tool's SARIF) is the
//! Phase 6 manifest provider. Findings become `results`, deduped rule ids become
//! `tool.driver.rules`, and severities map to SARIF levels.

use std::collections::BTreeSet;

use commenter_cat_core::comment::Comment;
use commenter_cat_core::error::{CommenterCatError, CommenterCatResult};
use commenter_cat_core::finding::Finding;
use commenter_cat_core::severity::Severity;
use serde_json::{json, Value};

use super::findings_with_context;

/// The SARIF 2.1.0 schema URI.
const SARIF_SCHEMA: &str = "https://json.schemastore.org/sarif-2.1.0.json";

/// Renders the check result as a SARIF 2.1.0 document (pretty-printed).
///
/// # Errors
/// Returns [`CommenterCatError::Render`] if serialization fails.
pub fn render(comments: &[Comment]) -> CommenterCatResult<String> {
    let pairs = findings_with_context(comments);
    let rules = rule_descriptors(&pairs);
    let results: Vec<Value> = pairs.iter().map(|(_, finding)| result(finding)).collect();

    let document = json!({
        "$schema": SARIF_SCHEMA,
        "version": "2.1.0",
        "runs": [{
            "tool": {
                "driver": {
                    "name": "commenter-cat",
                    "informationUri": "https://github.com/msaidbilgehan/Commenter-Cat",
                    "rules": rules,
                }
            },
            "results": results,
        }],
    });

    serde_json::to_string_pretty(&document)
        .map_err(|e| CommenterCatError::render("serializing SARIF").caused_by(e))
}

/// SARIF level for a severity (Critical and Error both gate the build → `error`).
fn sarif_level(severity: Severity) -> &'static str {
    match severity {
        Severity::Critical | Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "note",
    }
}

/// One SARIF `result` for a finding.
fn result(finding: &Finding) -> Value {
    json!({
        "ruleId": finding.provider_rule_id,
        "level": sarif_level(finding.severity),
        "message": { "text": finding.message },
        "locations": [{
            "physicalLocation": {
                "artifactLocation": { "uri": finding.file },
                "region": {
                    "startLine": finding.range.start_line,
                    "endLine": finding.range.end_line,
                },
            }
        }],
    })
}

/// The deduped `tool.driver.rules` array (first occurrence of each rule id wins).
fn rule_descriptors(pairs: &[(&Comment, &Finding)]) -> Vec<Value> {
    let mut seen = BTreeSet::new();
    let mut rules = Vec::new();
    for (_, finding) in pairs {
        if !seen.insert(finding.provider_rule_id.clone()) {
            continue;
        }
        let mut rule = json!({
            "id": finding.provider_rule_id,
            "name": finding.canonical_rule_id,
        });
        if let Some(url) = &finding.url {
            rule["helpUri"] = json!(url);
        }
        rules.push(rule);
    }
    rules
}

#[cfg(test)]
mod tests {
    use super::super::testkit::*;
    use super::*;
    use commenter_cat_core::finding::Category;

    #[test]
    fn test_emits_valid_sarif_structure() {
        let comments = vec![comment_with(vec![finding(
            "D417",
            Severity::Error,
            Category::DocDrift,
        )])];
        let sarif = render(&comments).unwrap();
        let value: Value = serde_json::from_str(&sarif).unwrap();

        assert_eq!(value["version"], "2.1.0");
        assert!(value["$schema"].is_string());
        assert_eq!(value["runs"][0]["tool"]["driver"]["name"], "commenter-cat");
        let result = &value["runs"][0]["results"][0];
        assert_eq!(result["ruleId"], "ruff:D417");
        assert_eq!(result["level"], "error");
        assert_eq!(
            result["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
            "pkg/app.py"
        );
        assert_eq!(
            result["locations"][0]["physicalLocation"]["region"]["startLine"],
            1
        );
        // The rule descriptor is present and deduped.
        assert_eq!(
            value["runs"][0]["tool"]["driver"]["rules"][0]["id"],
            "ruff:D417"
        );
    }

    #[test]
    fn test_severity_to_level_mapping() {
        assert_eq!(sarif_level(Severity::Critical), "error");
        assert_eq!(sarif_level(Severity::Warning), "warning");
        assert_eq!(sarif_level(Severity::Info), "note");
    }
}
