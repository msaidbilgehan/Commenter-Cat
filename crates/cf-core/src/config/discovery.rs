//! Config discovery: walk-up cascade, XDG global layer, and `CF_*` env overrides
//! (Idea §12, task 1.4).
//!
//! ## Precedence (highest wins)
//!
//! 1. `CF_*` environment variables.
//! 2. `comment-finder.toml` files, **nearest directory first** (walk-up cascade).
//! 3. The XDG global config (`$XDG_CONFIG_HOME/comment-finder/config.toml`).
//! 4. Built-in defaults (applied by [`ConfigFile::resolve`]).
//!
//! The filesystem/env-reading entry points ([`discover`], [`from_env`]) are thin
//! wrappers over pure, injectable cores ([`discover_in`], [`from_env_with`]) so
//! the cascade is tested without mutating process-global state.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::error::{CfError, CfResult};
use crate::kind::CommentKind;
use crate::lang::Language;
use crate::severity::Severity;
use crate::version::CONFIG_VERSION;

use super::model::{
    ConfigFile, EmbeddingsMode, MarkersSection, MergeOver, OnMissing, OutputFormat, OutputSection,
    ProvidersSection, ResolvedConfig, ScanSection, SearchSection, SeveritySection,
};

/// The per-directory config filename (sits beside `comment-finder.baseline.toml`,
/// Idea §5; outside the gitignored `.comment-finder/` cache, Idea §6).
pub const CONFIG_FILENAME: &str = "comment-finder.toml";

/// The XDG global config sub-path under the config home directory.
pub const GLOBAL_CONFIG_SUBPATH: &str = "comment-finder/config.toml";

/// The environment-variable prefix for config overrides (`CF_<SECTION>_<FIELD>`).
pub const ENV_PREFIX: &str = "CF_";

// --- Public entry points -----------------------------------------------------

/// Discovers and resolves configuration for a scan rooted at `start_dir`,
/// reading real environment variables and the real XDG global config.
pub fn discover(start_dir: &Path) -> CfResult<ResolvedConfig> {
    let get_env = |key: &str| std::env::var(key).ok();
    let global = global_config_path(&get_env);
    discover_in(start_dir, &get_env, global.as_deref())
}

/// The injectable core of [`discover`]: takes an explicit environment getter and
/// an explicit (optional) global-config path so the full cascade is testable.
pub fn discover_in(
    start_dir: &Path,
    get_env: &impl Fn(&str) -> Option<String>,
    global_path: Option<&Path>,
) -> CfResult<ResolvedConfig> {
    let env_layer = from_env_with(get_env)?;

    let mut file_layers = Vec::new();
    for path in walk_up_config_paths(start_dir) {
        file_layers.push(load_file(&path)?);
    }

    let global_layer = match global_path {
        Some(path) if path.is_file() => Some(load_file(path)?),
        _ => None,
    };

    Ok(assemble(env_layer, file_layers, global_layer))
}

/// Loads and validates a single config file into a partial [`ConfigFile`].
///
/// # Errors
/// Returns a [`CfError::Config`] if the file cannot be read, is not valid TOML,
/// or declares an unknown *future* [`CONFIG_VERSION`] (Idea §11).
pub fn load_file(path: &Path) -> CfResult<ConfigFile> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        CfError::config(format!("reading config at {}", path.display())).caused_by(e)
    })?;
    let file: ConfigFile = toml::from_str(&text).map_err(|e| {
        CfError::config(format!("parsing config at {}", path.display())).caused_by(e)
    })?;
    validate_version(&file, path)?;
    Ok(file)
}

/// Builds a config layer from real `CF_*` environment variables.
///
/// # Errors
/// Returns a [`CfError::Config`] if any present override fails to parse.
pub fn from_env() -> CfResult<ConfigFile> {
    from_env_with(&|key: &str| std::env::var(key).ok())
}

// --- Cascade assembly --------------------------------------------------------

/// Merges the layers by precedence and resolves to a concrete config.
///
/// `file_layers` are ordered nearest-directory-first; the env layer outranks all
/// files, and the global layer is the lowest-precedence base.
fn assemble(
    env_layer: ConfigFile,
    file_layers_nearest_first: Vec<ConfigFile>,
    global_layer: Option<ConfigFile>,
) -> ResolvedConfig {
    let mut acc = env_layer;
    for layer in file_layers_nearest_first {
        acc = acc.merge_over(layer);
    }
    if let Some(global) = global_layer {
        acc = acc.merge_over(global);
    }
    acc.resolve()
}

/// Returns existing `comment-finder.toml` paths from `start` up to the
/// filesystem root, **nearest first**.
fn walk_up_config_paths(start: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for dir in start.ancestors() {
        let candidate = dir.join(CONFIG_FILENAME);
        if candidate.is_file() {
            found.push(candidate);
        }
    }
    found
}

/// Resolves the XDG global config path from the environment, if determinable.
fn global_config_path(get_env: &impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    if let Some(xdg) = env_value(get_env, "XDG_CONFIG_HOME") {
        return Some(PathBuf::from(xdg).join(GLOBAL_CONFIG_SUBPATH));
    }
    #[cfg(windows)]
    if let Some(appdata) = env_value(get_env, "APPDATA") {
        return Some(PathBuf::from(appdata).join(GLOBAL_CONFIG_SUBPATH));
    }
    if let Some(home) = env_value(get_env, "HOME") {
        return Some(
            PathBuf::from(home)
                .join(".config")
                .join(GLOBAL_CONFIG_SUBPATH),
        );
    }
    None
}

/// Hard-errors if a config declares a version newer than this build supports
/// (Idea §11: "unknown future version = hard error with guidance").
fn validate_version(file: &ConfigFile, path: &Path) -> CfResult<()> {
    if let Some(version) = file.version {
        if version > CONFIG_VERSION {
            return Err(CfError::config(format!(
                "config at {} declares version {version}, but this cf supports up to {CONFIG_VERSION}; \
                 upgrade cf or lower the config version",
                path.display()
            )));
        }
    }
    Ok(())
}

// --- Environment parsing -----------------------------------------------------

/// Reads, trims, and empties-to-`None` a single environment variable.
fn env_value(get_env: &impl Fn(&str) -> Option<String>, key: &str) -> Option<String> {
    get_env(key)
        .map(|raw| raw.trim().to_owned())
        .filter(|trimmed| !trimmed.is_empty())
}

/// The injectable core of [`from_env`].
fn from_env_with(get_env: &impl Fn(&str) -> Option<String>) -> CfResult<ConfigFile> {
    let scan = ScanSection {
        respect_gitignore: env_bool(get_env, "CF_SCAN_RESPECT_GITIGNORE")?,
        languages: env_typed_list(
            get_env,
            "CF_SCAN_LANGUAGES",
            Language::from_token,
            &tokens_of(&Language::ALL.map(|l| l.as_str())),
        )?,
        extra_ignores: env_string_list(get_env, "CF_SCAN_EXTRA_IGNORES"),
        suppress_kinds: env_typed_list(
            get_env,
            "CF_SCAN_SUPPRESS_KINDS",
            CommentKind::from_token,
            &tokens_of(&CommentKind::ALL.map(|k| k.as_str())),
        )?,
    };
    let providers = ProvidersSection {
        python: env_value(get_env, "CF_PROVIDERS_PYTHON"),
        javascript: env_value(get_env, "CF_PROVIDERS_JAVASCRIPT"),
        typescript: env_value(get_env, "CF_PROVIDERS_TYPESCRIPT"),
        shell: env_value(get_env, "CF_PROVIDERS_SHELL"),
        secrets: env_value(get_env, "CF_PROVIDERS_SECRETS"),
        on_missing: env_enum(
            get_env,
            "CF_PROVIDERS_ON_MISSING",
            OnMissing::from_token,
            "warn, error",
        )?,
        docstring_convention: env_value(get_env, "CF_PROVIDERS_DOCSTRING_CONVENTION"),
    };
    let markers = MarkersSection {
        custom: env_string_list(get_env, "CF_MARKERS_CUSTOM"),
        // Map-valued overrides (marker→severity) are file-only by design.
        severity: None,
    };
    let severity = SeveritySection {
        fail_on: env_enum(
            get_env,
            "CF_SEVERITY_FAIL_ON",
            Severity::from_token,
            &tokens_of(&Severity::ALL.map(|s| s.as_str())),
        )?,
        // Rule/category overrides are file-only by design.
        overrides: BTreeMap::new(),
    };
    let search = SearchSection {
        embeddings: env_enum(
            get_env,
            "CF_SEARCH_EMBEDDINGS",
            EmbeddingsMode::from_token,
            "local",
        )?,
    };
    let output = OutputSection {
        default_format: env_enum(
            get_env,
            "CF_OUTPUT_DEFAULT_FORMAT",
            OutputFormat::from_token,
            &tokens_of(&[
                OutputFormat::Terminal.as_str(),
                OutputFormat::Jsonl.as_str(),
                OutputFormat::Sarif.as_str(),
                OutputFormat::Markdown.as_str(),
                OutputFormat::Csv.as_str(),
            ]),
        )?,
    };

    Ok(ConfigFile {
        version: None,
        scan: Some(scan),
        providers: Some(providers),
        markers: Some(markers),
        severity: Some(severity),
        search: Some(search),
        output: Some(output),
    })
}

/// Renders a list of valid tokens for an error hint.
fn tokens_of(tokens: &[&str]) -> String {
    tokens.join(", ")
}

/// Parses a boolean env override (`true`/`false`, case-insensitive).
fn env_bool(get_env: &impl Fn(&str) -> Option<String>, key: &str) -> CfResult<Option<bool>> {
    match env_value(get_env, key) {
        None => Ok(None),
        Some(raw) => match raw.to_ascii_lowercase().as_str() {
            "true" => Ok(Some(true)),
            "false" => Ok(Some(false)),
            other => Err(CfError::config(format!(
                "env {key}: expected 'true' or 'false', got '{other}'"
            ))),
        },
    }
}

/// Parses an enum-valued env override via its `from_token` constructor.
fn env_enum<T>(
    get_env: &impl Fn(&str) -> Option<String>,
    key: &str,
    parse: impl Fn(&str) -> Option<T>,
    valid: &str,
) -> CfResult<Option<T>> {
    match env_value(get_env, key) {
        None => Ok(None),
        Some(raw) => parse(&raw).map(Some).ok_or_else(|| {
            CfError::config(format!("env {key}: unknown value '{raw}'; valid: {valid}"))
        }),
    }
}

/// Parses a comma-separated string list, or `None` if unset.
fn env_string_list(get_env: &impl Fn(&str) -> Option<String>, key: &str) -> Option<Vec<String>> {
    env_value(get_env, key).map(|raw| split_csv(&raw))
}

/// Parses a comma-separated list of enum tokens, or `None` if unset.
fn env_typed_list<T>(
    get_env: &impl Fn(&str) -> Option<String>,
    key: &str,
    parse: impl Fn(&str) -> Option<T>,
    valid: &str,
) -> CfResult<Option<Vec<T>>> {
    let Some(raw) = env_value(get_env, key) else {
        return Ok(None);
    };
    let mut out = Vec::new();
    for token in split_csv(&raw) {
        let parsed = parse(&token).ok_or_else(|| {
            CfError::config(format!(
                "env {key}: unknown value '{token}'; valid: {valid}"
            ))
        })?;
        out.push(parsed);
    }
    Ok(Some(out))
}

/// Splits on commas, trimming each element and dropping empties.
fn split_csv(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A self-cleaning temporary directory (pure std — avoids a `tempfile` dep).
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new() -> Self {
            static COUNTER: AtomicU32 = AtomicU32::new(0);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!("cf-core-cfg-{}-{n}", std::process::id()));
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn write(&self, rel: &str, contents: &str) -> PathBuf {
            let target = self.path.join(rel);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&target, contents).unwrap();
            target
        }

        fn dir(&self, rel: &str) -> PathBuf {
            let target = self.path.join(rel);
            std::fs::create_dir_all(&target).unwrap();
            target
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn no_env() -> impl Fn(&str) -> Option<String> {
        |_| None
    }

    fn env_map(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |key: &str| map.get(key).cloned()
    }

    #[test]
    fn test_walk_up_cascade_nearer_dir_wins() {
        let tmp = TempDir::new();
        tmp.write(
            "comment-finder.toml",
            "version = 1\n[output]\ndefault_format = \"terminal\"\n[scan]\nrespect_gitignore = false\n",
        );
        let child = tmp.dir("pkg/sub");
        tmp.write(
            "pkg/sub/comment-finder.toml",
            "version = 1\n[output]\ndefault_format = \"jsonl\"\n",
        );

        let cfg = discover_in(&child, &no_env(), None).unwrap();
        // Child overrides the parent's output format...
        assert_eq!(cfg.output.default_format, OutputFormat::Jsonl);
        // ...but inherits the parent's scan setting (not set in the child).
        assert!(!cfg.scan.respect_gitignore);
    }

    #[test]
    fn test_env_overrides_file() {
        let tmp = TempDir::new();
        tmp.write(
            "comment-finder.toml",
            "version = 1\n[output]\ndefault_format = \"terminal\"\n",
        );
        let env = env_map(&[("CF_OUTPUT_DEFAULT_FORMAT", "jsonl")]);

        let cfg = discover_in(&tmp.path, &env, None).unwrap();
        assert_eq!(
            cfg.output.default_format,
            OutputFormat::Jsonl,
            "CF_* must outrank the file"
        );
    }

    #[test]
    fn test_global_is_lowest_precedence() {
        let tmp = TempDir::new();
        let global = tmp.write("global.toml", "version = 1\n[search]\nembeddings = \"local\"\n[output]\ndefault_format = \"markdown\"\n");
        let project = tmp.dir("project");
        tmp.write(
            "project/comment-finder.toml",
            "version = 1\n[output]\ndefault_format = \"csv\"\n",
        );

        let cfg = discover_in(&project, &no_env(), Some(&global)).unwrap();
        // Project file beats the global for the field both set...
        assert_eq!(cfg.output.default_format, OutputFormat::Csv);
        // ...and the global still contributes where the project is silent.
        assert_eq!(cfg.search.embeddings, EmbeddingsMode::Local);
    }

    #[test]
    fn test_unknown_future_version_is_hard_error() {
        let tmp = TempDir::new();
        let future = CONFIG_VERSION + 1;
        tmp.write("comment-finder.toml", &format!("version = {future}\n"));

        let err = discover_in(&tmp.path, &no_env(), None).unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains(&future.to_string()),
            "should name the bad version: {message}"
        );
        assert!(
            message.contains("upgrade cf"),
            "should give guidance: {message}"
        );
    }

    #[test]
    fn test_env_typed_list_and_bool_parse() {
        let env = env_map(&[
            ("CF_SCAN_LANGUAGES", "python, shell"),
            ("CF_SCAN_RESPECT_GITIGNORE", "FALSE"),
        ]);
        let cfg = discover_in(Path::new("/nonexistent-root-xyz"), &env, None).unwrap();
        assert_eq!(cfg.scan.languages, vec![Language::Python, Language::Shell]);
        assert!(!cfg.scan.respect_gitignore);
    }

    #[test]
    fn test_env_unknown_enum_value_errors_with_hint() {
        let env = env_map(&[("CF_OUTPUT_DEFAULT_FORMAT", "yaml")]);
        let err = from_env_with(&env).unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains("yaml"),
            "should name the bad value: {message}"
        );
        assert!(
            message.contains("terminal"),
            "should list valid tokens: {message}"
        );
    }

    #[test]
    fn test_no_config_anywhere_yields_defaults() {
        // A directory guaranteed to hold no config, no env, no global.
        let cfg = discover_in(Path::new("/nonexistent-root-xyz"), &no_env(), None).unwrap();
        assert_eq!(cfg, ResolvedConfig::default());
    }

    #[test]
    fn test_global_path_prefers_xdg() {
        let env = env_map(&[("XDG_CONFIG_HOME", "/tmp/xdg"), ("HOME", "/home/u")]);
        let path = global_config_path(&env).unwrap();
        assert!(path.ends_with(GLOBAL_CONFIG_SUBPATH));
        assert!(path.starts_with("/tmp/xdg"));
    }
}
