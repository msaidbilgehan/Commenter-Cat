//! The domain error hierarchy (general.md `ERR_*`).
//!
//! [`CfError`] is the single error type every Commenter-Cat crate returns. Its
//! variants are grouped by subsystem (config, walk, extract, map, storage,
//! provider, apply, identity); each carries a context-rich `message` and an
//! optional cause chain via `#[source]` ([`CfError::caused_by`]).
//!
//! ## Why explicit wrapping, not blanket `#[from]`
//!
//! Infrastructure errors (`rusqlite::Error`, subprocess failures, `gix` errors)
//! are **translated at the `cf-engine` adapter boundary** into a `CfError` with
//! operation context — never imported into this domain crate
//! (`ARCH_LAYER_VIOLATION`). A blanket `#[from]` would drop that context
//! (`ERR_NO_CONTEXT`); instead callers write
//! `op().map_err(|e| CfError::storage("opening index.db").caused_by(e))`, which
//! preserves the original via the `#[source]` chain (`ERR_BARE_RAISE`) while
//! adding the operation and sanitized inputs.

use std::fmt;

/// A boxed, thread-safe cause for the `#[source]` chain. Boxing keeps `cf-core`
/// free of infrastructure crates while preserving the original error.
pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// The workspace-wide result alias.
pub type CfResult<T> = Result<T, CfError>;

/// The root domain error for Commenter-Cat (general.md `ERR_*`).
///
/// Marked `#[non_exhaustive]` (enum and variants) so new subsystems and new
/// context fields can be added without breaking downstream `match` arms — pin a
/// `..` rest pattern when matching (`API_BREAKING_CHANGE`).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CfError {
    /// Loading, parsing, or validating configuration (Idea §12).
    #[error("configuration error: {message}")]
    #[non_exhaustive]
    Config {
        /// Operation context plus sanitized inputs (e.g. the config path).
        message: String,
        /// The underlying cause, if any (e.g. a TOML parse error).
        #[source]
        source: Option<BoxError>,
    },

    /// Walking the file tree / honoring ignore files (Idea §3, the `ignore` crate).
    #[error("file-walk error: {message}")]
    #[non_exhaustive]
    Walk {
        /// Operation context plus sanitized inputs (e.g. the root path).
        message: String,
        /// The underlying cause, if any.
        #[source]
        source: Option<BoxError>,
    },

    /// Extracting or classifying comments with tree-sitter (Idea §3).
    #[error("extraction error: {message}")]
    #[non_exhaustive]
    Extract {
        /// Operation context plus sanitized inputs (e.g. file and language).
        message: String,
        /// The underlying cause, if any (e.g. a grammar/parse failure).
        #[source]
        source: Option<BoxError>,
    },

    /// Mapping a comment to the code it annotates (Idea §3).
    #[error("mapping error: {message}")]
    #[non_exhaustive]
    Map {
        /// Operation context plus sanitized inputs.
        message: String,
        /// The underlying cause, if any.
        #[source]
        source: Option<BoxError>,
    },

    /// The two-layer SQLite cache (`inputs.db` / `index.db`, Idea §6).
    #[error("storage error: {message}")]
    #[non_exhaustive]
    Storage {
        /// Operation context plus sanitized inputs (e.g. which db / table).
        message: String,
        /// The underlying cause, if any (e.g. a `rusqlite` error, boxed here).
        #[source]
        source: Option<BoxError>,
    },

    /// Driving an external rule provider and normalizing its output (Idea §5).
    #[error("provider '{provider}' error: {message}")]
    #[non_exhaustive]
    Provider {
        /// The provider whose invocation failed (e.g. `"ruff"`, `"eslint"`).
        provider: String,
        /// Operation context plus sanitized inputs.
        message: String,
        /// The underlying cause, if any (e.g. spawn failure, malformed JSON).
        #[source]
        source: Option<BoxError>,
    },

    /// The parse-invariant safe-write path (Idea §4a, §5).
    #[error("apply error: {message}")]
    #[non_exhaustive]
    Apply {
        /// Operation context plus sanitized inputs.
        message: String,
        /// The underlying cause, if any.
        #[source]
        source: Option<BoxError>,
    },

    /// Cross-scan comment identity and matching (Idea §4).
    #[error("identity error: {message}")]
    #[non_exhaustive]
    Identity {
        /// Operation context plus sanitized inputs.
        message: String,
        /// The underlying cause, if any.
        #[source]
        source: Option<BoxError>,
    },

    /// Git enrichment: repo discovery, blame, changed-file sets (Idea §3, §7).
    #[error("git error: {message}")]
    #[non_exhaustive]
    Git {
        /// Operation context plus sanitized inputs (e.g. the file being blamed).
        message: String,
        /// The underlying cause, if any (e.g. a `gix` error, boxed here).
        #[source]
        source: Option<BoxError>,
    },

    /// Rendering output or parsing the canonical JSONL stream (Idea §8).
    #[error("render error: {message}")]
    #[non_exhaustive]
    Render {
        /// Operation context plus sanitized inputs (e.g. the output format).
        message: String,
        /// The underlying cause, if any (e.g. a serde serialization error).
        #[source]
        source: Option<BoxError>,
    },
}

impl CfError {
    /// A configuration error (Idea §12).
    pub fn config(message: impl Into<String>) -> Self {
        Self::Config {
            message: message.into(),
            source: None,
        }
    }

    /// A file-walk error (Idea §3).
    pub fn walk(message: impl Into<String>) -> Self {
        Self::Walk {
            message: message.into(),
            source: None,
        }
    }

    /// A comment-extraction error (Idea §3).
    pub fn extract(message: impl Into<String>) -> Self {
        Self::Extract {
            message: message.into(),
            source: None,
        }
    }

    /// A comment→code mapping error (Idea §3).
    pub fn map(message: impl Into<String>) -> Self {
        Self::Map {
            message: message.into(),
            source: None,
        }
    }

    /// A storage / cache error (Idea §6).
    pub fn storage(message: impl Into<String>) -> Self {
        Self::Storage {
            message: message.into(),
            source: None,
        }
    }

    /// A provider-invocation error, tagged with the provider name (Idea §5).
    pub fn provider(provider: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Provider {
            provider: provider.into(),
            message: message.into(),
            source: None,
        }
    }

    /// A safe-apply error (Idea §4a, §5).
    pub fn apply(message: impl Into<String>) -> Self {
        Self::Apply {
            message: message.into(),
            source: None,
        }
    }

    /// A cross-scan identity error (Idea §4).
    pub fn identity(message: impl Into<String>) -> Self {
        Self::Identity {
            message: message.into(),
            source: None,
        }
    }

    /// A git-enrichment error (Idea §3, §7).
    pub fn git(message: impl Into<String>) -> Self {
        Self::Git {
            message: message.into(),
            source: None,
        }
    }

    /// A render / output-stream error (Idea §8).
    pub fn render(message: impl Into<String>) -> Self {
        Self::Render {
            message: message.into(),
            source: None,
        }
    }

    /// Attaches an underlying cause, preserving the original via the `#[source]`
    /// chain (`ERR_BARE_RAISE`). Idiomatic at adapter seams:
    ///
    /// ```
    /// # use cf_core::error::CfError;
    /// let io = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
    /// let err = CfError::config("reading commenter-cat.toml").caused_by(io);
    /// assert!(std::error::Error::source(&err).is_some());
    /// ```
    #[must_use]
    pub fn caused_by(mut self, source: impl Into<BoxError>) -> Self {
        let slot = match &mut self {
            CfError::Config { source, .. }
            | CfError::Walk { source, .. }
            | CfError::Extract { source, .. }
            | CfError::Map { source, .. }
            | CfError::Storage { source, .. }
            | CfError::Provider { source, .. }
            | CfError::Apply { source, .. }
            | CfError::Identity { source, .. }
            | CfError::Git { source, .. }
            | CfError::Render { source, .. } => source,
        };
        *slot = Some(source.into());
        self
    }

    /// The subsystem label for this error, for structured logging and grouping.
    #[must_use]
    pub const fn subsystem(&self) -> &'static str {
        match self {
            CfError::Config { .. } => "config",
            CfError::Walk { .. } => "walk",
            CfError::Extract { .. } => "extract",
            CfError::Map { .. } => "map",
            CfError::Storage { .. } => "storage",
            CfError::Provider { .. } => "provider",
            CfError::Apply { .. } => "apply",
            CfError::Identity { .. } => "identity",
            CfError::Git { .. } => "git",
            CfError::Render { .. } => "render",
        }
    }
}

/// Renders the full cause chain as ` -> a -> b -> c`, for log lines and tests.
///
/// The standard `Display` shows only the top error; this walks `source()` so
/// the root cause is visible without a backtrace.
#[must_use]
pub fn cause_chain(err: &CfError) -> String {
    use std::error::Error;
    let mut out = err.to_string();
    let mut current = err.source();
    while let Some(cause) = current {
        out.push_str(" -> ");
        let _ = fmt::write(&mut out, format_args!("{cause}"));
        current = cause.source();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn test_display_includes_subsystem_and_message() {
        let err = CfError::config("missing [scan] table");
        assert_eq!(err.to_string(), "configuration error: missing [scan] table");
        assert_eq!(err.subsystem(), "config");
    }

    #[test]
    fn test_provider_variant_names_the_provider() {
        let err = CfError::provider("ruff", "spawn failed");
        assert_eq!(err.to_string(), "provider 'ruff' error: spawn failed");
        assert_eq!(err.subsystem(), "provider");
    }

    #[test]
    fn test_caused_by_preserves_source_chain() {
        let io = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let err = CfError::storage("opening index.db").caused_by(io);
        let source = err.source().expect("source must be set");
        assert!(source.to_string().contains("denied"));
    }

    #[test]
    fn test_cause_chain_walks_to_root() {
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "no such file");
        let err = CfError::walk("scanning repo root").caused_by(io);
        let chain = cause_chain(&err);
        assert_eq!(chain, "file-walk error: scanning repo root -> no such file");
    }

    #[test]
    fn test_no_source_when_unset() {
        let err = CfError::map("orphan comment");
        assert!(err.source().is_none());
        assert_eq!(cause_chain(&err), "mapping error: orphan comment");
    }

    #[test]
    fn test_error_is_send_and_sync() {
        // CfError must cross thread boundaries (rayon native pass, async MCP).
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CfError>();
    }
}
