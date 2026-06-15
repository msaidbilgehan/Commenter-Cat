//! Config data model — on-disk (partial) vs. resolved (concrete).
//!
//! Two representations, deliberately distinct (task 1.4):
//!
//! * **Partial** ([`ConfigFile`] and its `*Section` structs) — every field is
//!   `Option`, so a layer expresses *only what it overrides*. Partials merge
//!   with [`MergeOver`] (higher precedence wins per field) before defaults are
//!   applied. This is what `cf-core` deserializes from each TOML layer and what
//!   the env layer produces.
//! * **Resolved** ([`ResolvedConfig`] and its `*Config` structs) — every field
//!   is concrete. Produced once, after the whole cascade is merged, by
//!   [`ConfigFile::resolve`]. This is what the rest of the engine consumes; it
//!   can never carry an "unset" field.
//!
//! Defaults are named constants (general.md `ORG_MAGIC_NUMBER`) drawn from the
//! Idea §12 configuration sketch.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::kind::CommentKind;
use crate::lang::Language;
use crate::severity::Severity;
use crate::version::CONFIG_VERSION;

// --- Defaults (Idea §12) -----------------------------------------------------

const DEFAULT_RESPECT_GITIGNORE: bool = true;
const DEFAULT_SUPPRESS_KINDS: [CommentKind; 2] = [CommentKind::License, CommentKind::Shebang];
const DEFAULT_ON_MISSING: OnMissing = OnMissing::Warn;
const DEFAULT_FAIL_ON: Severity = Severity::Error;
const DEFAULT_OUTPUT_FORMAT: OutputFormat = OutputFormat::Terminal;
const DEFAULT_EMBEDDINGS: EmbeddingsMode = EmbeddingsMode::Local;
const DEFAULT_PYTHON_PROVIDER: &str = "ruff";
const DEFAULT_JAVASCRIPT_PROVIDER: &str = "eslint";
const DEFAULT_TYPESCRIPT_PROVIDER: &str = "eslint";
const DEFAULT_SHELL_PROVIDER: &str = "shellcheck";
const DEFAULT_SECRETS_PROVIDER: &str = "gitleaks";

// --- Scalar enums ------------------------------------------------------------

/// Behavior when a configured provider binary is absent (Idea §5, §12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OnMissing {
    /// Skip the language's deep rules, warn, stay non-fatal (graceful degradation).
    Warn,
    /// Treat an absent provider as a hard error.
    Error,
}

impl OnMissing {
    const ALL: [OnMissing; 2] = [OnMissing::Warn, OnMissing::Error];

    /// The lowercase config/CLI token (matches the serde form).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            OnMissing::Warn => "warn",
            OnMissing::Error => "error",
        }
    }

    /// Parses a token into a value, or `None` if unrecognized.
    #[must_use]
    pub fn from_token(token: &str) -> Option<OnMissing> {
        OnMissing::ALL.into_iter().find(|v| v.as_str() == token)
    }
}

/// Default render format for non-agent output (Idea §8, §12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    /// Human terminal output, grouped by tag/file/author/age.
    Terminal,
    /// Canonical machine-readable JSONL stream (Idea §4b).
    Jsonl,
    /// SARIF for code-scanning (Idea §8).
    Sarif,
    /// Markdown report.
    Markdown,
    /// CSV export.
    Csv,
}

impl OutputFormat {
    const ALL: [OutputFormat; 5] = [
        OutputFormat::Terminal,
        OutputFormat::Jsonl,
        OutputFormat::Sarif,
        OutputFormat::Markdown,
        OutputFormat::Csv,
    ];

    /// The lowercase config/CLI token (matches the serde form).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            OutputFormat::Terminal => "terminal",
            OutputFormat::Jsonl => "jsonl",
            OutputFormat::Sarif => "sarif",
            OutputFormat::Markdown => "markdown",
            OutputFormat::Csv => "csv",
        }
    }

    /// Parses a token into a value, or `None` if unrecognized.
    #[must_use]
    pub fn from_token(token: &str) -> Option<OutputFormat> {
        OutputFormat::ALL.into_iter().find(|v| v.as_str() == token)
    }
}

/// Embedding backend (Idea §6, §12). Local ONNX only — comments are proprietary
/// context and never leave the machine (Idea §13 rejects cloud embeddings).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EmbeddingsMode {
    /// `fastembed-rs` / `ort` ONNX, computed on-device.
    Local,
}

impl EmbeddingsMode {
    const ALL: [EmbeddingsMode; 1] = [EmbeddingsMode::Local];

    /// The lowercase config/CLI token (matches the serde form).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            EmbeddingsMode::Local => "local",
        }
    }

    /// Parses a token into a value, or `None` if unrecognized.
    #[must_use]
    pub fn from_token(token: &str) -> Option<EmbeddingsMode> {
        EmbeddingsMode::ALL
            .into_iter()
            .find(|v| v.as_str() == token)
    }
}

// --- Merge trait -------------------------------------------------------------

/// Field-wise override merge: `self` (higher precedence) wins over `base`.
pub trait MergeOver {
    /// Returns a value taking each set field from `self`, falling back to `base`.
    #[must_use]
    fn merge_over(self, base: Self) -> Self;
}

/// Merges two optional layers: if both are present, merge them; otherwise keep
/// whichever exists, preferring the higher-precedence `hi`.
fn merge_opt<T: MergeOver>(hi: Option<T>, lo: Option<T>) -> Option<T> {
    match (hi, lo) {
        (Some(h), Some(l)) => Some(h.merge_over(l)),
        (h, l) => h.or(l),
    }
}

// --- Partial (on-disk) model -------------------------------------------------

/// One configuration layer as written on disk or assembled from the
/// environment — every field optional so layers compose (task 1.4).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ConfigFile {
    /// Schema version; validated against [`CONFIG_VERSION`] at load time.
    pub version: Option<u32>,
    /// `[scan]` — what to walk and which kinds to keep.
    pub scan: Option<ScanSection>,
    /// `[providers]` — external rule providers per language.
    pub providers: Option<ProvidersSection>,
    /// `[markers]` — custom markers and their severities.
    pub markers: Option<MarkersSection>,
    /// `[severity]` — canonical-severity overrides plus the CI `fail_on` gate.
    pub severity: Option<SeveritySection>,
    /// `[search]` — embedding backend selection.
    pub search: Option<SearchSection>,
    /// `[output]` — default render format.
    pub output: Option<OutputSection>,
}

/// Partial `[scan]` table.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ScanSection {
    /// Honor `.gitignore` / `.ignore` during the walk (Idea §3).
    pub respect_gitignore: Option<bool>,
    /// Languages to extract (Idea §3).
    pub languages: Option<Vec<Language>>,
    /// Extra ignore globs layered onto gitignore.
    pub extra_ignores: Option<Vec<String>>,
    /// Comment kinds suppressed from default views (Idea §3).
    pub suppress_kinds: Option<Vec<CommentKind>>,
}

impl MergeOver for ScanSection {
    fn merge_over(self, base: Self) -> Self {
        Self {
            respect_gitignore: self.respect_gitignore.or(base.respect_gitignore),
            languages: self.languages.or(base.languages),
            extra_ignores: self.extra_ignores.or(base.extra_ignores),
            suppress_kinds: self.suppress_kinds.or(base.suppress_kinds),
        }
    }
}

/// Partial `[providers]` table.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProvidersSection {
    /// Provider id for Python (default `ruff`).
    pub python: Option<String>,
    /// Provider id for JavaScript (default `eslint`).
    pub javascript: Option<String>,
    /// Provider id for TypeScript (default `eslint`).
    pub typescript: Option<String>,
    /// Provider id for Shell (default `shellcheck`).
    pub shell: Option<String>,
    /// Provider id for secrets scanning (default `gitleaks`).
    pub secrets: Option<String>,
    /// Behavior when a provider binary is absent (Idea §5).
    pub on_missing: Option<OnMissing>,
    /// Docstring convention passed through to the provider, e.g. `"google"`.
    pub docstring_convention: Option<String>,
}

impl MergeOver for ProvidersSection {
    fn merge_over(self, base: Self) -> Self {
        Self {
            python: self.python.or(base.python),
            javascript: self.javascript.or(base.javascript),
            typescript: self.typescript.or(base.typescript),
            shell: self.shell.or(base.shell),
            secrets: self.secrets.or(base.secrets),
            on_missing: self.on_missing.or(base.on_missing),
            docstring_convention: self.docstring_convention.or(base.docstring_convention),
        }
    }
}

/// Partial `[markers]` table.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct MarkersSection {
    /// Custom marker tokens beyond the native set (Idea §3, §12).
    pub custom: Option<Vec<String>>,
    /// Per-marker canonical severity, e.g. `DO_NOT_MERGE = "critical"`.
    pub severity: Option<BTreeMap<String, Severity>>,
}

impl MergeOver for MarkersSection {
    fn merge_over(self, base: Self) -> Self {
        Self {
            custom: self.custom.or(base.custom),
            severity: self.severity.or(base.severity),
        }
    }
}

/// Partial `[severity]` table: the reserved `fail_on` gate plus a flattened map
/// of canonical-severity overrides keyed by rule id, category, or origin
/// (Idea §5).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SeveritySection {
    /// CI gate — fail the build at or above this level (Idea §5).
    pub fail_on: Option<Severity>,
    /// Overrides keyed by `provider_rule_id` / `category` / `origin`. Any key
    /// other than `fail_on` lands here (Idea §5).
    #[serde(flatten)]
    pub overrides: BTreeMap<String, Severity>,
}

impl MergeOver for SeveritySection {
    fn merge_over(self, base: Self) -> Self {
        // Override maps union; the higher-precedence layer wins per key.
        let mut overrides = base.overrides;
        overrides.extend(self.overrides);
        Self {
            fail_on: self.fail_on.or(base.fail_on),
            overrides,
        }
    }
}

/// Partial `[search]` table.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SearchSection {
    /// Embedding backend (Idea §6).
    pub embeddings: Option<EmbeddingsMode>,
}

impl MergeOver for SearchSection {
    fn merge_over(self, base: Self) -> Self {
        Self {
            embeddings: self.embeddings.or(base.embeddings),
        }
    }
}

/// Partial `[output]` table.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct OutputSection {
    /// Default render format (Idea §8).
    pub default_format: Option<OutputFormat>,
}

impl MergeOver for OutputSection {
    fn merge_over(self, base: Self) -> Self {
        Self {
            default_format: self.default_format.or(base.default_format),
        }
    }
}

impl MergeOver for ConfigFile {
    fn merge_over(self, base: Self) -> Self {
        Self {
            version: self.version.or(base.version),
            scan: merge_opt(self.scan, base.scan),
            providers: merge_opt(self.providers, base.providers),
            markers: merge_opt(self.markers, base.markers),
            severity: merge_opt(self.severity, base.severity),
            search: merge_opt(self.search, base.search),
            output: merge_opt(self.output, base.output),
        }
    }
}

impl ConfigFile {
    /// Collapses this fully-merged partial into a concrete [`ResolvedConfig`],
    /// applying the Idea §12 defaults for every field still unset.
    ///
    /// Infallible: per-file validation (version, enum tokens) happens at load
    /// and parse time, so by the time the cascade is resolved every remaining
    /// decision is "value or default".
    #[must_use]
    pub fn resolve(self) -> ResolvedConfig {
        let scan = self.scan.unwrap_or_default();
        let providers = self.providers.unwrap_or_default();
        let markers = self.markers.unwrap_or_default();
        let severity = self.severity.unwrap_or_default();
        let search = self.search.unwrap_or_default();
        let output = self.output.unwrap_or_default();

        ResolvedConfig {
            version: self.version.unwrap_or(CONFIG_VERSION),
            scan: ScanConfig {
                respect_gitignore: scan.respect_gitignore.unwrap_or(DEFAULT_RESPECT_GITIGNORE),
                languages: scan.languages.unwrap_or_else(|| Language::ALL.to_vec()),
                extra_ignores: scan.extra_ignores.unwrap_or_default(),
                suppress_kinds: scan
                    .suppress_kinds
                    .unwrap_or_else(|| DEFAULT_SUPPRESS_KINDS.to_vec()),
            },
            providers: ProvidersConfig {
                python: providers
                    .python
                    .or_else(|| Some(DEFAULT_PYTHON_PROVIDER.to_owned())),
                javascript: providers
                    .javascript
                    .or_else(|| Some(DEFAULT_JAVASCRIPT_PROVIDER.to_owned())),
                typescript: providers
                    .typescript
                    .or_else(|| Some(DEFAULT_TYPESCRIPT_PROVIDER.to_owned())),
                shell: providers
                    .shell
                    .or_else(|| Some(DEFAULT_SHELL_PROVIDER.to_owned())),
                secrets: providers
                    .secrets
                    .or_else(|| Some(DEFAULT_SECRETS_PROVIDER.to_owned())),
                on_missing: providers.on_missing.unwrap_or(DEFAULT_ON_MISSING),
                docstring_convention: providers.docstring_convention,
            },
            markers: MarkersConfig {
                custom: markers.custom.unwrap_or_default(),
                severity: markers.severity.unwrap_or_default(),
            },
            severity: SeverityConfig {
                fail_on: severity.fail_on.unwrap_or(DEFAULT_FAIL_ON),
                overrides: severity.overrides,
            },
            search: SearchConfig {
                embeddings: search.embeddings.unwrap_or(DEFAULT_EMBEDDINGS),
            },
            output: OutputConfig {
                default_format: output.default_format.unwrap_or(DEFAULT_OUTPUT_FORMAT),
            },
        }
    }
}

// --- Resolved (concrete) model -----------------------------------------------

/// The fully-resolved configuration the engine consumes. Every field is
/// concrete; there is no "unset" state (task 1.4).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolvedConfig {
    /// The config schema version this resolved from.
    pub version: u32,
    /// Resolved `[scan]` settings.
    pub scan: ScanConfig,
    /// Resolved `[providers]` settings.
    pub providers: ProvidersConfig,
    /// Resolved `[markers]` settings.
    pub markers: MarkersConfig,
    /// Resolved `[severity]` settings.
    pub severity: SeverityConfig,
    /// Resolved `[search]` settings.
    pub search: SearchConfig,
    /// Resolved `[output]` settings.
    pub output: OutputConfig,
}

impl Default for ResolvedConfig {
    /// The all-defaults configuration (equivalent to an empty config file).
    fn default() -> Self {
        ConfigFile::default().resolve()
    }
}

/// Resolved `[scan]` settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScanConfig {
    /// Honor `.gitignore` / `.ignore` during the walk.
    pub respect_gitignore: bool,
    /// Languages to extract.
    pub languages: Vec<Language>,
    /// Extra ignore globs layered onto gitignore.
    pub extra_ignores: Vec<String>,
    /// Comment kinds suppressed from default views.
    pub suppress_kinds: Vec<CommentKind>,
}

/// Resolved `[providers]` settings. A `None` provider means that language has
/// no configured rule provider (native facts still apply).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProvidersConfig {
    /// Provider id for Python.
    pub python: Option<String>,
    /// Provider id for JavaScript.
    pub javascript: Option<String>,
    /// Provider id for TypeScript.
    pub typescript: Option<String>,
    /// Provider id for Shell.
    pub shell: Option<String>,
    /// Provider id for secrets scanning.
    pub secrets: Option<String>,
    /// Behavior when a provider binary is absent.
    pub on_missing: OnMissing,
    /// Docstring convention passed through to the provider.
    pub docstring_convention: Option<String>,
}

impl ProvidersConfig {
    /// The configured provider id for a language, if any (Idea §5).
    #[must_use]
    pub fn for_language(&self, language: Language) -> Option<&str> {
        let id = match language {
            Language::Python => &self.python,
            Language::JavaScript => &self.javascript,
            Language::TypeScript => &self.typescript,
            Language::Shell => &self.shell,
        };
        id.as_deref()
    }
}

/// Resolved `[markers]` settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MarkersConfig {
    /// Custom marker tokens beyond the native set.
    pub custom: Vec<String>,
    /// Per-marker canonical severity.
    pub severity: BTreeMap<String, Severity>,
}

/// Resolved `[severity]` settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SeverityConfig {
    /// CI gate — fail the build at or above this level.
    pub fail_on: Severity,
    /// Overrides keyed by `provider_rule_id` / `category` / `origin`.
    pub overrides: BTreeMap<String, Severity>,
}

/// Resolved `[search]` settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SearchConfig {
    /// Embedding backend.
    pub embeddings: EmbeddingsMode,
}

/// Resolved `[output]` settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OutputConfig {
    /// Default render format.
    pub default_format: OutputFormat,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_config_yields_idea_defaults() {
        let cfg = ResolvedConfig::default();
        assert_eq!(cfg.version, CONFIG_VERSION);
        assert!(cfg.scan.respect_gitignore);
        assert_eq!(cfg.scan.languages, Language::ALL.to_vec());
        assert_eq!(
            cfg.scan.suppress_kinds,
            vec![CommentKind::License, CommentKind::Shebang]
        );
        assert_eq!(cfg.providers.python.as_deref(), Some("ruff"));
        assert_eq!(cfg.providers.javascript.as_deref(), Some("eslint"));
        assert_eq!(cfg.providers.shell.as_deref(), Some("shellcheck"));
        assert_eq!(cfg.providers.secrets.as_deref(), Some("gitleaks"));
        assert_eq!(cfg.providers.on_missing, OnMissing::Warn);
        assert_eq!(cfg.severity.fail_on, Severity::Error);
        assert_eq!(cfg.search.embeddings, EmbeddingsMode::Local);
        assert_eq!(cfg.output.default_format, OutputFormat::Terminal);
    }

    #[test]
    fn test_merge_over_prefers_higher_precedence_per_field() {
        let hi = ConfigFile {
            output: Some(OutputSection {
                default_format: Some(OutputFormat::Jsonl),
            }),
            ..Default::default()
        };
        let lo = ConfigFile {
            version: Some(1),
            output: Some(OutputSection {
                default_format: Some(OutputFormat::Terminal),
            }),
            scan: Some(ScanSection {
                respect_gitignore: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        };
        let resolved = hi.merge_over(lo).resolve();
        // hi wins on the field it set...
        assert_eq!(resolved.output.default_format, OutputFormat::Jsonl);
        // ...lo fills the rest.
        assert!(!resolved.scan.respect_gitignore);
    }

    #[test]
    fn test_severity_section_splits_fail_on_from_overrides() {
        let section: SeveritySection = toml::from_str(
            r#"
            fail_on = "error"
            doc_missing = "error"
            "ruff:D100" = "info"
            "#,
        )
        .unwrap();
        assert_eq!(section.fail_on, Some(Severity::Error));
        assert_eq!(section.overrides.get("doc_missing"), Some(&Severity::Error));
        assert_eq!(section.overrides.get("ruff:D100"), Some(&Severity::Info));
        assert!(
            !section.overrides.contains_key("fail_on"),
            "fail_on must not leak into overrides"
        );
    }

    #[test]
    fn test_full_sketch_parses() {
        // The exact Idea §12 configuration sketch must round-trip into a partial.
        let file: ConfigFile = toml::from_str(SKETCH).unwrap();
        let cfg = file.resolve();
        assert_eq!(cfg.scan.languages.len(), 4);
        assert_eq!(
            cfg.providers.docstring_convention.as_deref(),
            Some("google")
        );
        assert_eq!(cfg.markers.custom, vec!["SECURITY", "DO_NOT_MERGE"]);
        assert_eq!(
            cfg.markers.severity.get("DO_NOT_MERGE"),
            Some(&Severity::Critical)
        );
        assert_eq!(
            cfg.severity.overrides.get("ruff:D100"),
            Some(&Severity::Info)
        );
        assert_eq!(cfg.severity.fail_on, Severity::Error);
    }

    #[test]
    fn test_providers_for_language() {
        let cfg = ResolvedConfig::default();
        assert_eq!(cfg.providers.for_language(Language::Python), Some("ruff"));
        assert_eq!(
            cfg.providers.for_language(Language::TypeScript),
            Some("eslint")
        );
        assert_eq!(
            cfg.providers.for_language(Language::Shell),
            Some("shellcheck")
        );
    }

    const SKETCH: &str = r#"
version = 1

[scan]
respect_gitignore = true
languages = ["python", "typescript", "javascript", "shell"]
extra_ignores = ["vendor/", "*.generated.*"]
suppress_kinds = ["license", "shebang"]

[providers]
python = "ruff"
javascript = "eslint"
typescript = "eslint"
shell = "shellcheck"
secrets = "gitleaks"
on_missing = "warn"
docstring_convention = "google"

[markers]
custom = ["SECURITY", "DO_NOT_MERGE"]
severity = { SECURITY = "error", DO_NOT_MERGE = "critical" }

[severity]
doc_missing = "error"
"ruff:D100" = "info"
fail_on = "error"

[search]
embeddings = "local"

[output]
default_format = "terminal"
"#;
}
