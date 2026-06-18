//! The eslint Tier-2 native provider (Idea §5; task 6.6).
//!
//! eslint is the **documented exception** to manifest-first: its Node stack
//! needs runtime behavior (project/TS-program scope, a pinned dependency *tree*),
//! so it is a hand-written [`RuleProvider`]. It drives eslint +
//! `eslint-plugin-jsdoc` (+ tsdoc) in JSON mode, ingests a curated comment-rule
//! subset, and converts eslint's **UTF-16 columns** to byte offsets via the
//! Phase-3 coordinate layer (Idea §5 risk R6). It records its
//! [`ReproducibilityLevel`] per run.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use commenter_cat_core::error::CommenterCatResult;
use commenter_cat_core::finding::coordinates::CoordinateSystem;
use commenter_cat_core::finding::{
    resolve_severity, Category, Finding, FindingTarget, Fix, Origin, Range,
};
use commenter_cat_core::lang::Language;
use commenter_cat_core::symbol::BoundSymbol;

use super::node_runtime::{NodeRuntime, ReproducibilityLevel};
use crate::provider::contract::{Capabilities, ProviderContext, ProviderRun, RuleProvider, Scope};
use crate::walk::to_repo_relative;

/// The provider id.
const ID: &str = "eslint";

/// The languages eslint handles (Idea §5) — the orchestrator narrows its file
/// set to these, so it never lints a `.py`/`.sh` file.
const LANGUAGES: [Language; 2] = [Language::TypeScript, Language::JavaScript];

/// The eslint Tier-2 native provider.
pub struct EslintProvider {
    runtime: NodeRuntime,
    capabilities: Capabilities,
}

impl EslintProvider {
    /// Builds the provider for a given Node-runtime tier.
    #[must_use]
    pub fn new(runtime: NodeRuntime) -> Self {
        let capabilities = Capabilities {
            // Project-scoped: TS type-aware rules need the whole program.
            scope: Scope::Project,
            supports_fix: true,
            supports_incremental: false,
            supports_sarif: false,
            // eslint lints code, not just comments (Idea §5: comment-scoping is
            // gitleaks' secrets-in-comments opt-in, not a general default).
            comment_scoped: false,
            // eslint reports 1-based UTF-16 columns (Idea §5).
            coordinate_system: CoordinateSystem::eslint(),
        };
        Self {
            runtime,
            capabilities,
        }
    }

    /// The reproducibility guarantee recorded for runs (Idea §5).
    #[must_use]
    pub fn reproducibility_level(&self) -> ReproducibilityLevel {
        self.runtime.reproducibility_level()
    }

    /// Normalizes recorded eslint JSON into canonical findings, converting
    /// UTF-16 columns to byte offsets via `file_text`. Pure — no subprocess.
    ///
    /// # Errors
    /// Returns [`commenter_cat_core::CommenterCatError`] only on an internal conversion failure
    /// (currently infallible; the signature mirrors the provider contract).
    pub fn normalize<F: Fn(&str) -> Option<String>>(
        &self,
        output: &Value,
        context: &ProviderContext<'_>,
        file_text: F,
    ) -> CommenterCatResult<Vec<Finding>> {
        let mut findings = Vec::new();

        let Some(file_results) = output.as_array() else {
            return Ok(findings);
        };
        for file_result in file_results {
            let path = file_result
                .get("filePath")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let repo_path = to_repo_relative(Path::new(path), context.root);
            let text = file_text(&repo_path);

            let Some(messages) = file_result.get("messages").and_then(Value::as_array) else {
                continue;
            };
            for message in messages {
                // A null ruleId is a parse error, not a comment finding.
                let Some(rule_id) = message.get("ruleId").and_then(Value::as_str) else {
                    continue;
                };
                // Curated comment-domain subset only (Idea §5) — drop the rest.
                let Some(category) = comment_category(rule_id) else {
                    continue;
                };
                findings.push(self.build_finding(
                    rule_id,
                    category,
                    message,
                    &repo_path,
                    text.as_deref(),
                    context,
                ));
            }
        }
        findings.sort_by(|a, b| a.canonical_sort_key().cmp(&b.canonical_sort_key()));
        Ok(findings)
    }

    fn build_finding(
        &self,
        rule_id: &str,
        category: Category,
        message: &Value,
        repo_path: &str,
        text: Option<&str>,
        context: &ProviderContext<'_>,
    ) -> Finding {
        let coord = self.capabilities.coordinate_system;
        let origin = Origin::Eslint;
        let provider_rule_id = Finding::qualify_rule_id(&origin, rule_id);
        let canonical_rule_id = rule_id.to_owned();
        let severity = resolve_severity(
            category,
            &provider_rule_id,
            &canonical_rule_id,
            &origin,
            context.severity_overrides,
        );

        let line = u32_at(message, "line").unwrap_or(1);
        let column = u32_at(message, "column");
        let start_byte = text
            .and_then(|t| coord.to_byte_offset(t, line, column.unwrap_or(coord.column_base())))
            .unwrap_or(0);

        Finding {
            file: repo_path.to_owned(),
            target: FindingTarget::Symbol(BoundSymbol::new(repo_path)),
            range: Range::new(start_byte, start_byte, line, line),
            origin,
            provider_rule_id,
            canonical_rule_id,
            category,
            severity,
            // eslint severity is numeric: 2 = error, 1 = warn (Idea §5 per-tool).
            severity_native: u32_at(message, "severity").map(|s| s.to_string()),
            message: message
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            fix: Fix::ProviderAutofix,
            url: None,
            also_from: std::collections::BTreeSet::new(),
        }
    }
}

impl RuleProvider for EslintProvider {
    fn id(&self) -> &str {
        ID
    }

    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn languages(&self) -> &[Language] {
        &LANGUAGES
    }

    fn run(&self, files: &[PathBuf], context: &ProviderContext<'_>) -> ProviderRun {
        // The pinned eslint binary path is resolved by provider management (6.7);
        // the baseline invocation drives the system/pinned `eslint` in JSON mode.
        let mut command = Command::new("eslint");
        command
            .arg("--format")
            .arg("json")
            .current_dir(context.root);
        for file in files {
            command.arg(file);
        }
        let output = match command.output() {
            Ok(output) => output,
            // Absent Node stack is graceful degradation (Idea §5).
            Err(_) => return ProviderRun::skipped(),
        };
        let Ok(json) = serde_json::from_slice::<Value>(&output.stdout) else {
            return ProviderRun::partial();
        };
        let root = context.root.to_path_buf();
        match self.normalize(&json, context, |path| {
            std::fs::read_to_string(root.join(path)).ok()
        }) {
            Ok(findings) => ProviderRun::ran(findings),
            Err(_) => ProviderRun::partial(),
        }
    }
}

/// The curated comment-domain category for an eslint rule, or `None` to drop it
/// (Idea §5: a curated subset, never eslint's full code-lint output).
fn comment_category(rule_id: &str) -> Option<Category> {
    match rule_id {
        "jsdoc/check-param-names" | "jsdoc/check-types" | "jsdoc/check-property-names" => {
            Some(Category::DocDrift)
        }
        "jsdoc/require-jsdoc"
        | "require-jsdoc"
        | "jsdoc/require-param"
        | "jsdoc/require-returns"
        | "jsdoc/require-description" => Some(Category::DocMissing),
        "no-warning-comments" => Some(Category::TodoFormat),
        "tsdoc/syntax"
        | "jsdoc/check-alignment"
        | "jsdoc/check-indentation"
        | "jsdoc/no-multi-asterisks"
        | "jsdoc/multiline-blocks" => Some(Category::DocStyle),
        _ => None,
    }
}

fn u32_at(value: &Value, key: &str) -> Option<u32> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use commenter_cat_core::severity::Severity;
    use serde_json::json;
    use std::collections::BTreeMap;

    #[test]
    fn test_jsdoc_findings_map_with_utf16_columns() {
        let provider = EslintProvider::new(NodeRuntime::SemiHermetic);
        let overrides = BTreeMap::new();
        let context = ProviderContext::new(Path::new("."), &overrides);
        // Recorded eslint JSON: one jsdoc finding + an out-of-domain rule.
        let output = json!([
            { "filePath": "a.ts", "messages": [
                { "ruleId": "jsdoc/check-param-names", "severity": 1,
                  "message": "@param 'y' does not match", "line": 1, "column": 3 },
                { "ruleId": "no-unused-vars", "severity": 2,
                  "message": "'x' is unused", "line": 1, "column": 1 }
            ]}
        ]);

        // File "💩x": 💩 is 2 UTF-16 units, so eslint column 3 is 'x' → byte 4.
        let findings = provider
            .normalize(&output, &context, |_| Some("💩x".to_owned()))
            .unwrap();
        assert_eq!(findings.len(), 1, "out-of-domain no-unused-vars is dropped");
        let finding = &findings[0];
        assert_eq!(finding.provider_rule_id, "eslint:jsdoc/check-param-names");
        assert_eq!(finding.category, Category::DocDrift);
        assert_eq!(
            finding.severity,
            Severity::Error,
            "doc_drift anchors to error"
        );
        assert_eq!(finding.severity_native.as_deref(), Some("1"));
        assert_eq!(
            finding.range.start_byte, 4,
            "UTF-16 column 3 → byte 4 (after 💩)"
        );
    }

    #[test]
    fn test_capabilities_and_reproducibility() {
        let semi = EslintProvider::new(NodeRuntime::SemiHermetic);
        assert_eq!(semi.id(), "eslint");
        assert_eq!(semi.capabilities().scope, Scope::Project);
        assert_eq!(
            semi.capabilities().coordinate_system,
            CoordinateSystem::eslint()
        );
        assert_eq!(
            semi.reproducibility_level(),
            ReproducibilityLevel::SemiHermetic
        );

        let hermetic = EslintProvider::new(NodeRuntime::Hermetic);
        assert_eq!(
            hermetic.reproducibility_level(),
            ReproducibilityLevel::Hermetic
        );
    }

    #[test]
    fn test_parse_error_message_without_ruleid_is_skipped() {
        let provider = EslintProvider::new(NodeRuntime::SemiHermetic);
        let overrides = BTreeMap::new();
        let context = ProviderContext::new(Path::new("."), &overrides);
        let output = json!([
            { "filePath": "a.ts", "messages": [
                { "ruleId": null, "severity": 2, "message": "Parsing error", "line": 1, "column": 1 }
            ]}
        ]);
        assert!(provider
            .normalize(&output, &context, |_| None)
            .unwrap()
            .is_empty());
    }
}
