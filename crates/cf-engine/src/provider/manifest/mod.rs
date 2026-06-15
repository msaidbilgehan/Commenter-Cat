//! Tier-1 manifest providers (Idea §5; tasks 6.3, 6.4).
//!
//! A declarative TOML adapter — no compilation, no scripting — covering ~80–90%
//! of integrations. The built-in [`ManifestProvider`] consumes a
//! `manifest_version = 1` manifest: a `command` to spawn, a `format`
//! (`json` via [`jsonpath`] or `sarif` via the generic [`sarif`] mapper), a
//! `scope`, declarative [`mapping`] tables, and a [`capabilities`] block.
//! **No embedded code** — that is the threshold to a Tier-2 native provider.

pub mod capabilities;
pub mod jsonpath;
pub mod mapping;
pub mod sarif;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use serde_json::Value;

use cf_core::error::{CfError, CfResult};
use cf_core::finding::coordinates::CoordinateSystem;
use cf_core::finding::{resolve_severity, Category, Finding, FindingTarget, Fix, Origin, Range};
use cf_core::severity::Severity;
use cf_core::symbol::BoundSymbol;
use cf_core::version::manifest_version_is_supported;

use super::contract::{Capabilities, ProviderContext, ProviderRun, RuleProvider, Scope};
use capabilities::ManifestCapabilities;
use jsonpath::FindingSpec;

/// The category assigned to a finding whose rule is not in `[category_map]`.
const DEFAULT_CATEGORY: Category = Category::CommentStyle;

/// The token in `command` replaced by the file list.
const FILES_TOKEN: &str = "{files}";

/// The token in `command` replaced by the scan root — for project-scoped tools
/// that take a single directory argument (e.g. `gitleaks dir {root}`) rather than
/// an explicit file list.
const ROOT_TOKEN: &str = "{root}";

/// Normalizes a tool-reported path to the repo-relative, `/`-separated form CF
/// uses for comment records, so a provider finding can attach to its comment
/// (Idea §4 — attachment is by `path` + range).
///
/// Tools echo the path they were handed, which CF passes as **absolute** — and
/// some tools (ruff) canonicalize it, so on macOS a `/tmp/...` root resurfaces as
/// `/private/tmp/...`. Matching that against `comment.path` (repo-relative) needs
/// canonicalization on *both* sides, with graceful fallbacks when a path cannot
/// be canonicalized (e.g. it no longer exists).
fn relativize(file: &str, root: &Path, canonical_root: &Path) -> String {
    let raw = Path::new(file);
    let absolute = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        root.join(raw)
    };
    let canonical_file = absolute.canonicalize().unwrap_or(absolute);
    let relative = canonical_file
        .strip_prefix(canonical_root)
        .or_else(|_| canonical_file.strip_prefix(root))
        .or_else(|_| raw.strip_prefix(root))
        .unwrap_or(raw);
    relative.to_string_lossy().replace('\\', "/")
}

/// The manifest's output format (Idea §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    /// Tool JSON, extracted via `[[findings]]` JSONPaths.
    Json,
    /// SARIF, mapped by the generic ingester (no field-paths).
    Sarif,
}

/// A Tier-1 declarative provider manifest (Idea §5).
#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    /// Manifest format version (read current + prior major, Idea §11).
    pub manifest_version: u32,
    /// The command to spawn (`{files}` expands to the file list).
    pub command: Vec<String>,
    /// Output format.
    pub format: Format,
    /// Invocation scope.
    pub scope: Scope,
    /// JSON extraction specs (unused for SARIF).
    #[serde(default)]
    pub findings: Vec<FindingSpec>,
    /// `[severity_map]` — native severity → canonical.
    #[serde(default)]
    pub severity_map: BTreeMap<String, Severity>,
    /// `[category_map]` — native rule id → canonical category.
    #[serde(default)]
    pub category_map: BTreeMap<String, Category>,
    /// Category for findings whose rule is not in `[category_map]` (e.g. every
    /// gitleaks finding is a `secret`). Falls back to `comment_style`.
    #[serde(default)]
    pub default_category: Option<Category>,
    /// When true, **drop** findings whose rule is not in `[category_map]` — the
    /// comment-domain filter that keeps a curated rule subset, never a tool's
    /// full code-lint output (Idea §5).
    #[serde(default)]
    pub keep_only_mapped: bool,
    /// `[capabilities]`.
    pub capabilities: ManifestCapabilities,
}

/// A provider driven entirely by a manifest (Idea §5).
pub struct ManifestProvider {
    id: String,
    manifest: Manifest,
    capabilities: Capabilities,
}

impl ManifestProvider {
    /// Loads a provider from manifest TOML under `id` (e.g. `"ruff"`).
    ///
    /// # Errors
    /// Returns [`CfError::Provider`] on a parse error or an unsupported
    /// `manifest_version`.
    pub fn from_toml(id: impl Into<String>, toml_str: &str) -> CfResult<Self> {
        let id = id.into();
        let manifest: Manifest = toml::from_str(toml_str)
            .map_err(|e| CfError::provider(&id, "parsing manifest").caused_by(e))?;
        if !manifest_version_is_supported(manifest.manifest_version) {
            return Err(CfError::provider(
                &id,
                format!("unsupported manifest_version {}", manifest.manifest_version),
            ));
        }
        let capabilities = manifest.capabilities.to_capabilities(manifest.scope);
        Ok(Self {
            id,
            manifest,
            capabilities,
        })
    }

    /// Extracts and normalizes findings from a tool's JSON `output`, reading file
    /// text via `file_text` for coordinate conversion. Pure — no subprocess.
    ///
    /// # Errors
    /// Returns [`CfError::Provider`] if a JSONPath in the manifest is invalid.
    pub fn normalize<F: Fn(&str) -> Option<String>>(
        &self,
        output: &Value,
        context: &ProviderContext<'_>,
        file_text: F,
    ) -> CfResult<Vec<Finding>> {
        let raw = match self.manifest.format {
            Format::Json => jsonpath::extract(&self.id, output, &self.manifest.findings)?,
            Format::Sarif => sarif::extract(output),
        };
        let origin = Origin::from_token(&self.id);
        let coord = self.capabilities.coordinate_system;
        // Canonicalize the root once; `relativize` reuses it per finding so the
        // tool's absolute (possibly canonicalized) paths match `comment.path`.
        let canonical_root = context
            .root
            .canonicalize()
            .unwrap_or_else(|_| context.root.to_path_buf());

        let mut findings = Vec::with_capacity(raw.len());
        for entry in &raw {
            let canonical_rule_id = entry.native_rule_id.clone();
            let provider_rule_id = Finding::qualify_rule_id(&origin, &entry.native_rule_id);
            let mapped =
                mapping::apply_category_map(&self.manifest.category_map, &entry.native_rule_id)
                    .or(self.manifest.default_category);
            let category = match mapped {
                Some(category) => category,
                // Out-of-domain finding under a curated manifest → drop it (§5).
                None if self.manifest.keep_only_mapped => continue,
                None => DEFAULT_CATEGORY,
            };
            // §5 resolution: config override → category anchor. The native
            // severity is recorded; `severity_map` is the per-tool fallback.
            let severity = resolve_severity(
                category,
                &provider_rule_id,
                &canonical_rule_id,
                &origin,
                context.severity_overrides,
            );
            // Repo-relative path so the finding attaches to its comment (Idea §4).
            let rel_file = relativize(&entry.file, context.root, &canonical_root);
            let range = self.range_for(coord, entry.line, entry.column, &file_text(&rel_file));

            findings.push(Finding {
                target: FindingTarget::Symbol(BoundSymbol::new(&rel_file)),
                file: rel_file,
                range,
                origin: origin.clone(),
                provider_rule_id,
                canonical_rule_id,
                category,
                severity,
                severity_native: entry.native_severity.clone(),
                message: entry.message.clone(),
                fix: if self.capabilities.supports_fix {
                    Fix::ProviderAutofix
                } else {
                    Fix::None
                },
                url: None,
                also_from: std::collections::BTreeSet::new(),
            });
        }
        findings.sort_by(|a, b| a.canonical_sort_key().cmp(&b.canonical_sort_key()));
        Ok(findings)
    }

    /// Converts a provider `(line, column)` to a [`Range`] using the declared
    /// coordinate system and the file text (byte offset `0` if text is absent).
    fn range_for(
        &self,
        coord: CoordinateSystem,
        line: u32,
        column: Option<u32>,
        file_text: &Option<String>,
    ) -> Range {
        let our_line = line.saturating_sub(coord.line_base()).saturating_add(1);
        let start_byte = file_text
            .as_ref()
            .and_then(|text| {
                coord.to_byte_offset(text, line, column.unwrap_or(coord.column_base()))
            })
            .unwrap_or(0);
        Range::new(start_byte, start_byte, our_line, our_line)
    }

    /// Expands `command`, replacing `{files}` with the file list and `{root}`
    /// with the scan root (for project-scoped tools that take a single dir).
    fn build_args(&self, files: &[PathBuf], root: &Path) -> Vec<String> {
        let mut args = Vec::new();
        for token in &self.manifest.command {
            match token.as_str() {
                FILES_TOKEN => args.extend(files.iter().map(|f| f.to_string_lossy().into_owned())),
                ROOT_TOKEN => args.push(root.to_string_lossy().into_owned()),
                _ => args.push(token.clone()),
            }
        }
        args
    }
}

impl RuleProvider for ManifestProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn run(&self, files: &[PathBuf], context: &ProviderContext<'_>) -> ProviderRun {
        let args = self.build_args(files, context.root);
        let Some((binary, rest)) = args.split_first() else {
            return ProviderRun::skipped();
        };
        // Argument array, never a shell (general.md SEC_COMMAND_INJECTION).
        let output = match Command::new(binary)
            .args(rest)
            .current_dir(context.root)
            .output()
        {
            Ok(output) => output,
            // A missing binary is graceful degradation, not failure (Idea §5).
            Err(_) => return ProviderRun::skipped(),
        };
        // Exit code is not the signal — parsed JSON = ran (Idea §5).
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::run_state::RunState;
    use serde_json::json;
    use std::path::Path;

    const RUFF_LIKE: &str = r#"
manifest_version = 1
command = ["ruff", "check", "--output-format", "json", "{files}"]
format = "json"
scope = "file"

[[findings]]
iterate = "$.results[*]"
native_rule_id = "$.code"
severity = "$.level"
message = "$.message"
file = "$.filename"
line = "$.location.row"
column = "$.location.column"

[severity_map]
error = "error"
warning = "warning"

[category_map]
D417 = "doc_drift"
ERA001 = "commented_code"

[capabilities]
supports_fix = true
coordinate_system = "1-based-utf8"
"#;

    #[test]
    fn test_normalizes_json_with_category_severity_and_byte_offset() {
        let provider = ManifestProvider::from_toml("ruff", RUFF_LIKE).unwrap();
        let output = json!({ "results": [
            { "code": "D417", "level": "warning", "message": "param drift",
              "filename": "a.py", "location": { "row": 1, "column": 3 } }
        ]});
        let overrides = BTreeMap::new();
        let context = ProviderContext::new(Path::new("."), &overrides);

        // File text "# hi": column 3 (1-based) → byte offset 2.
        let findings = provider
            .normalize(&output, &context, |_| Some("# hi".to_owned()))
            .unwrap();
        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert_eq!(finding.origin, Origin::Ruff);
        assert_eq!(finding.provider_rule_id, "ruff:D417");
        assert_eq!(finding.category, Category::DocDrift);
        assert_eq!(
            finding.severity,
            Severity::Error,
            "doc_drift anchors to error (§5)"
        );
        assert_eq!(finding.severity_native.as_deref(), Some("warning"));
        assert_eq!(finding.fix, Fix::ProviderAutofix);
        assert_eq!(finding.range.start_byte, 2, "1-based-utf8 col 3 → byte 2");
    }

    const SARIF_MANIFEST: &str = r#"
manifest_version = 1
command = ["tool", "--sarif", "{files}"]
format = "sarif"
scope = "file"

[category_map]
SC2086 = "comment_style"

[capabilities]
supports_sarif = true
coordinate_system = "1-based-utf8"
"#;

    #[test]
    fn test_normalizes_sarif_via_generic_mapper() {
        let provider = ManifestProvider::from_toml("shellcheck", SARIF_MANIFEST).unwrap();
        let sarif = json!({ "runs": [{ "results": [{
            "ruleId": "SC2086", "level": "warning", "message": { "text": "quote it" },
            "locations": [{ "physicalLocation": {
                "artifactLocation": { "uri": "a.sh" },
                "region": { "startLine": 1, "startColumn": 1 } } }]
        }]}]});
        let overrides = BTreeMap::new();
        let context = ProviderContext::new(Path::new("."), &overrides);

        let findings = provider.normalize(&sarif, &context, |_| None).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].provider_rule_id, "shellcheck:SC2086");
        assert_eq!(findings[0].category, Category::CommentStyle);
        assert!(provider.capabilities().supports_sarif);
    }

    const ABSENT: &str = r#"
manifest_version = 1
command = ["cf-nonexistent-binary-xyzzy", "{files}"]
format = "json"
scope = "file"
[capabilities]
coordinate_system = "1-based-utf8"
"#;

    #[test]
    fn test_absent_binary_is_skipped() {
        let provider = ManifestProvider::from_toml("ghost", ABSENT).unwrap();
        let overrides = BTreeMap::new();
        let context = ProviderContext::new(Path::new("."), &overrides);
        let run = provider.run(&[PathBuf::from("a.py")], &context);
        assert_eq!(run.state, RunState::Skipped);
    }

    const ROOT_SCOPED: &str = r#"
manifest_version = 1
command = ["gitleaks", "dir", "--report-path", "-", "{root}"]
format = "json"
scope = "project"
default_category = "secret"
[[findings]]
iterate = "$[*]"
native_rule_id = "$.RuleID"
message = "$.Description"
file = "$.File"
line = "$.StartLine"
column = "$.StartColumn"
[capabilities]
coordinate_system = "1-based-utf8"
"#;

    #[test]
    fn test_build_args_expands_root_token() {
        let provider = ManifestProvider::from_toml("gitleaks", ROOT_SCOPED).unwrap();
        let args = provider.build_args(&[PathBuf::from("ignored.py")], Path::new("/repo/here"));
        // `{root}` → the single scan root; `{files}` is absent, so the file list
        // is not appended (gitleaks takes one path, not a list).
        assert_eq!(
            args,
            vec!["gitleaks", "dir", "--report-path", "-", "/repo/here"]
        );
    }

    #[test]
    fn test_normalize_relativizes_absolute_tool_paths() {
        // The bug dogfooding surfaced: ruff/gitleaks echo the ABSOLUTE path CF
        // hands them, which never equals the repo-relative `comment.path`, so no
        // finding ever attached. After the fix `finding.file` is repo-relative.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir(dir.path().join("pkg")).unwrap();
        let file = dir.path().join("pkg/app.py");
        std::fs::write(&file, "# x = old()\n").unwrap();

        let provider = ManifestProvider::from_toml("ruff", RUFF_LIKE).unwrap();
        let overrides = BTreeMap::new();
        let context = ProviderContext::new(dir.path(), &overrides);
        // ruff reports the canonicalized absolute path it was given.
        let absolute = file.canonicalize().unwrap();
        let output = json!({ "results": [
            { "code": "ERA001", "level": "warning", "message": "commented code",
              "filename": absolute.to_string_lossy(), "location": { "row": 1, "column": 1 } }
        ]});

        let findings = provider
            .normalize(&output, &context, |path| {
                std::fs::read_to_string(dir.path().join(path)).ok()
            })
            .unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].file, "pkg/app.py",
            "absolute tool path normalized to repo-relative"
        );
        // The target symbol is the same repo-relative path (attachment hinge).
        match &findings[0].target {
            FindingTarget::Symbol(symbol) => assert_eq!(symbol.as_str(), "pkg/app.py"),
            other => panic!("expected symbol target, got {other:?}"),
        }
    }

    #[test]
    fn test_unsupported_manifest_version_errors() {
        let toml = "manifest_version = 99\ncommand=[\"x\"]\nformat=\"json\"\nscope=\"file\"\n[capabilities]\ncoordinate_system=\"1-based-utf8\"\n";
        assert!(ManifestProvider::from_toml("x", toml).is_err());
    }
}
