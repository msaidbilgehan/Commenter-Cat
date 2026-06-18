//! The docstring↔signature diff (detector 2; task 3.3).
//!
//! Diffs the *claimed* [`DocContract`] against the *real* [`RealSignature`] and
//! emits a `rot_signature` finding for each disagreement:
//!
//! * a **documented-but-absent parameter** — the canonical catch, e.g. a
//!   docstring still listing `timeout` after the parameter was renamed to
//!   `timeout_s`;
//! * a **documented return** the function no longer provides (conservative);
//! * a **documented raise** when the function raises nothing at all
//!   (conservative — a function that raises *something* is never flagged, since
//!   the documented type may surface via a helper).
//!
//! Acts only on `DocContract`-intent comments bound to a function; everything
//! else (and an empty contract) no-ops.

use std::path::Path;

use commenter_cat_core::comment::Comment;
use commenter_cat_core::finding::{Category, Finding};

use crate::ops::triage::native_finding;
use crate::rot::intent::CommentIntent;
use crate::rot::signature::doc_contract::parse_doc_contract;
use crate::rot::signature::sig_extract::extract_signature;

/// The stable canonical rule id for signature-contract findings.
const RULE: &str = "rot_signature";

/// The `rot_signature` findings for a comment — the documented-vs-real signature
/// disagreements. Empty unless the comment is a doc contract bound to a function.
#[must_use]
pub fn signature_findings(comment: &Comment, source: &str, intent: CommentIntent) -> Vec<Finding> {
    if !intent.is_doc_contract() {
        return Vec::new();
    }
    let Some(node_range) = comment.bound_node_range else {
        return Vec::new();
    };
    let contract = parse_doc_contract(&comment.raw_text);
    if contract.is_empty() {
        return Vec::new();
    }
    let path = Path::new(&comment.path);
    let Some(real) = extract_signature(source, comment.language, path, node_range) else {
        return Vec::new();
    };

    let mut findings = Vec::new();
    for param in &contract.params {
        if !real.params.contains(param) {
            findings.push(drift(
                comment,
                format!(
                    "docstring documents parameter `{param}`, which is not in the function signature"
                ),
            ));
        }
    }
    if contract.documents_return && !real.has_return {
        findings.push(drift(
            comment,
            "docstring documents a return value, but the function returns nothing".to_owned(),
        ));
    }
    if real.raises.is_empty() {
        for exception in &contract.raises {
            findings.push(drift(
                comment,
                format!(
                    "docstring documents raising `{exception}`, but the function raises nothing"
                ),
            ));
        }
    }
    findings
}

/// Builds a `DocDrift` finding anchored to the whole doc comment.
fn drift(comment: &Comment, message: String) -> Finding {
    native_finding(
        comment,
        comment.range,
        Category::DocDrift,
        RULE.to_owned(),
        Category::DocDrift.canonical_severity(),
        message,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::coalesce::coalesce;
    use crate::extract::extract_source;
    use crate::map::map_comments;
    use crate::rot::intent::classify_intent;
    use commenter_cat_core::lang::Language;

    fn findings_for(source: &str, language: Language, file: &str) -> Vec<Finding> {
        let path = Path::new(file);
        let mut comments = coalesce(
            source,
            extract_source(source, language, path, file).unwrap(),
        );
        map_comments(source, language, path, &mut comments).unwrap();
        comments
            .iter()
            .flat_map(|comment| signature_findings(comment, source, classify_intent(comment)))
            .collect()
    }

    fn messages(findings: &[Finding]) -> Vec<String> {
        findings.iter().map(|f| f.message.clone()).collect()
    }

    #[test]
    fn test_renamed_parameter_is_flagged() {
        // The canonical catch: docstring says `timeout`, signature has `timeout_s`.
        let source = "def connect(timeout_s):\n    \"\"\"Open a connection.\n\n    Args:\n        timeout: seconds to wait\n    \"\"\"\n    return do_connect(timeout_s)\n";
        let findings = findings_for(source, Language::Python, "m.py");
        assert_eq!(findings.len(), 1, "{:?}", messages(&findings));
        assert_eq!(findings[0].canonical_rule_id, RULE);
        assert_eq!(findings[0].category, Category::DocDrift);
        assert!(findings[0].message.contains("timeout`"));
    }

    #[test]
    fn test_accurate_docstring_is_not_flagged() {
        let source = "def connect(timeout_s):\n    \"\"\"Open a connection.\n\n    Args:\n        timeout_s: seconds to wait\n    \"\"\"\n    return do_connect(timeout_s)\n";
        let findings = findings_for(source, Language::Python, "m.py");
        assert!(findings.is_empty(), "{:?}", messages(&findings));
    }

    #[test]
    fn test_documented_return_that_does_not_exist() {
        let source = "def store(key, value):\n    \"\"\"Persist a value.\n\n    Returns:\n        Nothing really.\n    \"\"\"\n    cache[key] = value\n";
        let findings = findings_for(source, Language::Python, "m.py");
        assert_eq!(findings.len(), 1, "{:?}", messages(&findings));
        assert!(findings[0].message.contains("return value"));
    }

    #[test]
    fn test_documented_raise_with_no_raise_in_body() {
        let source = "def validate(x):\n    \"\"\"Check.\n\n    Raises:\n        ValueError: when bad\n    \"\"\"\n    return x > 0\n";
        let findings = findings_for(source, Language::Python, "m.py");
        assert_eq!(findings.len(), 1, "{:?}", messages(&findings));
        assert!(findings[0].message.contains("ValueError"));
    }

    #[test]
    fn test_documented_raise_is_not_flagged_when_body_raises() {
        // Conservative: a function that raises *something* never trips raise-drift.
        let source = "def validate(x):\n    \"\"\"Check.\n\n    Raises:\n        ValueError: when bad\n    \"\"\"\n    if not x:\n        raise ValueError('bad')\n    return True\n";
        let findings = findings_for(source, Language::Python, "m.py");
        assert!(findings.is_empty(), "{:?}", messages(&findings));
    }

    #[test]
    fn test_jsdoc_renamed_param_is_flagged() {
        let source = "/**\n * Sends it.\n * @param {string} recipient - who\n */\nexport function send(to: string): void {\n    deliver(to);\n}\n";
        let findings = findings_for(source, Language::TypeScript, "a.ts");
        assert_eq!(findings.len(), 1, "{:?}", messages(&findings));
        assert!(findings[0].message.contains("recipient"));
    }

    #[test]
    fn test_non_doc_contract_comment_is_ignored() {
        // A plain bound line comment is not a doc contract → no signature check.
        let source = "def f(a):\n    # Args:\n    #   b: not really a contract\n    return a\n";
        let findings = findings_for(source, Language::Python, "m.py");
        assert!(findings.is_empty(), "{:?}", messages(&findings));
    }
}
