//! Dogfooded built-in provider manifests (Idea §5; task 6.5).
//!
//! The three single-binary built-ins ship as **manifests**, loaded exactly like
//! a user adapter — proving the manifest format is a real platform, not a
//! privileged built-in path (Idea §5 dogfooding rule). eslint is the documented
//! exception (Tier-2 native, task 6.6).

use cf_core::error::CfResult;

use super::manifest::ManifestProvider;

const RUFF_MANIFEST: &str = include_str!("../../assets/providers/ruff.manifest.toml");
const SHELLCHECK_MANIFEST: &str = include_str!("../../assets/providers/shellcheck.manifest.toml");
const GITLEAKS_MANIFEST: &str = include_str!("../../assets/providers/gitleaks.manifest.toml");

/// The built-in `(id, manifest)` pairs, embedded in the binary.
pub const BUILTIN_MANIFESTS: [(&str, &str); 3] = [
    ("ruff", RUFF_MANIFEST),
    ("shellcheck", SHELLCHECK_MANIFEST),
    ("gitleaks", GITLEAKS_MANIFEST),
];

/// Loads a built-in provider by id, or `None` if there is no such built-in.
///
/// # Errors
/// Returns [`cf_core::CfError`] if the embedded manifest fails to parse.
pub fn load_builtin(id: &str) -> Option<CfResult<ManifestProvider>> {
    BUILTIN_MANIFESTS
        .iter()
        .find(|(name, _)| *name == id)
        .map(|(name, toml)| ManifestProvider::from_toml(*name, toml))
}

/// Loads every built-in provider.
///
/// # Errors
/// Returns [`cf_core::CfError`] if any embedded manifest fails to parse.
pub fn load_all() -> CfResult<Vec<ManifestProvider>> {
    BUILTIN_MANIFESTS
        .iter()
        .map(|(name, toml)| ManifestProvider::from_toml(*name, toml))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::contract::ProviderContext;
    use crate::provider::RuleProvider;
    use cf_core::finding::Category;
    use cf_core::severity::Severity;
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::path::Path;

    fn context() -> BTreeMap<String, Severity> {
        BTreeMap::new()
    }

    #[test]
    fn test_all_builtins_load() {
        let providers = load_all().unwrap();
        assert_eq!(providers.len(), 3);
        let ids: Vec<&str> = providers.iter().map(RuleProvider::id).collect();
        assert_eq!(ids, vec!["ruff", "shellcheck", "gitleaks"]);
    }

    #[test]
    fn test_ruff_fixture_maps_curated_subset() {
        let ruff = load_builtin("ruff").unwrap().unwrap();
        let overrides = context();
        let ctx = ProviderContext::new(Path::new("."), &overrides);
        // A recorded ruff JSON: D417 (in domain) + E501 (out of domain → dropped).
        let output = json!([
            { "code": "D417", "message": "Missing argument descriptions in the docstring",
              "filename": "a.py", "location": { "row": 1, "column": 1 } },
            { "code": "E501", "message": "Line too long",
              "filename": "a.py", "location": { "row": 2, "column": 1 } }
        ]);
        let findings = ruff.normalize(&output, &ctx, |_| None).unwrap();
        assert_eq!(findings.len(), 1, "out-of-domain E501 is dropped");
        assert_eq!(findings[0].provider_rule_id, "ruff:D417");
        assert_eq!(findings[0].category, Category::DocDrift);
        assert_eq!(findings[0].severity, Severity::Error);
    }

    #[test]
    fn test_shellcheck_fixture_numeric_code() {
        let shellcheck = load_builtin("shellcheck").unwrap().unwrap();
        let overrides = context();
        let ctx = ProviderContext::new(Path::new("."), &overrides);
        let output = json!([
            { "code": 2148, "level": "error", "message": "shebang missing",
              "file": "a.sh", "line": 1, "column": 1 }
        ]);
        let findings = shellcheck.normalize(&output, &ctx, |_| None).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].provider_rule_id, "shellcheck:2148");
        assert_eq!(findings[0].category, Category::Shebang);
        assert_eq!(findings[0].severity_native.as_deref(), Some("error"));
    }

    #[test]
    fn test_gitleaks_fixture_is_always_secret_critical() {
        let gitleaks = load_builtin("gitleaks").unwrap().unwrap();
        let overrides = context();
        let ctx = ProviderContext::new(Path::new("."), &overrides);
        let output = json!([
            { "RuleID": "aws-access-key", "Description": "AWS Access Key",
              "File": "config.py", "StartLine": 5, "StartColumn": 10 }
        ]);
        let findings = gitleaks.normalize(&output, &ctx, |_| None).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].provider_rule_id, "gitleaks:aws-access-key");
        assert_eq!(findings[0].category, Category::Secret);
        assert_eq!(
            findings[0].severity,
            Severity::Critical,
            "secret anchors to critical"
        );
    }

    #[test]
    fn test_unknown_builtin_is_none() {
        assert!(
            load_builtin("eslint").is_none(),
            "eslint is Tier-2 native, not a built-in manifest"
        );
    }
}
