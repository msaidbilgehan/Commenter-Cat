//! Suppression export (Idea §5; task 7.7).
//!
//! `cf suppressions export` is the **opt-in, one-way inverse of filter-up**: it
//! materializes the suppression set into each tool's native directives
//! (`# noqa: D417`, `// eslint-disable-next-line`, `# shellcheck disable=…`) for
//! teams that also run the tools directly. It is written **through the
//! parse-invariant applier** so source edits stay safe. The default flow never
//! touches source.

use std::path::Path;

use cf_core::error::CfResult;
use cf_core::finding::Origin;
use cf_core::lang::Language;

use crate::ops::apply::parse_invariant;

/// The native suppression directive text for an `(origin, rule)`, or `None` for
/// origins with no native directive (Idea §5).
#[must_use]
pub fn native_directive(origin: &Origin, rule: &str) -> Option<String> {
    match origin {
        Origin::Ruff => Some(format!("# noqa: {rule}")),
        Origin::Eslint => Some(format!("// eslint-disable-next-line {rule}")),
        Origin::Shellcheck => Some(format!("# shellcheck disable={rule}")),
        Origin::Gitleaks => Some("# gitleaks:allow".to_owned()),
        // Native CF findings and third-party providers have no native directive.
        Origin::Native | Origin::Other(_) => None,
    }
}

/// Materializes `directive` into `source` at `byte_pos`, through the
/// parse-invariant applier (safe writes).
///
/// # Errors
/// Returns [`cf_core::CfError`] if the insertion would alter code.
pub fn export_directive(
    source: &str,
    language: Language,
    path: &Path,
    byte_pos: usize,
    directive: &str,
) -> CfResult<String> {
    parse_invariant::insert_comment(source, language, path, byte_pos, directive)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_per_tool_native_directives() {
        assert_eq!(
            native_directive(&Origin::Ruff, "D417").as_deref(),
            Some("# noqa: D417")
        );
        assert_eq!(
            native_directive(&Origin::Eslint, "jsdoc/check-param-names").as_deref(),
            Some("// eslint-disable-next-line jsdoc/check-param-names")
        );
        assert_eq!(
            native_directive(&Origin::Shellcheck, "SC2086").as_deref(),
            Some("# shellcheck disable=SC2086")
        );
        assert_eq!(native_directive(&Origin::Native, "rot").as_deref(), None);
    }

    #[test]
    fn test_export_inserts_directive_via_parse_invariance() {
        // Append `  # noqa: D417` to the end of the statement line.
        let source = "x = 1\n";
        let directive = native_directive(&Origin::Ruff, "D417").unwrap();
        let exported = export_directive(
            source,
            Language::Python,
            Path::new("a.py"),
            5,
            &format!("  {directive}"),
        )
        .unwrap();
        assert_eq!(exported, "x = 1  # noqa: D417\n");
    }

    #[test]
    fn test_export_refuses_to_inject_code() {
        // Inserting code (not a comment) is rejected by parse-invariance.
        let source = "x = 1\n";
        assert!(
            export_directive(source, Language::Python, Path::new("a.py"), 5, "; evil()").is_err()
        );
    }
}
