//! Provider pinning + cache layout (Idea §5; task 6.7).
//!
//! Pinned + auto-managed is the **default and canonical** mode: pinned versions
//! are fetched/cached at `~/.cache/commenter-cat/providers/` (single-binary tools cleanly;
//! the eslint Node stack pins a full lockfile tree). `--system-tools` is the
//! explicit escape hatch, and **baselines may only be updated under pinned
//! tools** (Idea §5). The actual fetch is an adapter-boundary concern; this
//! module owns the pinning *state* and cache *layout*.

use std::path::{Path, PathBuf};

/// A pinned provider version — a SemVer scalar for single-binary tools, or a
/// lockfile-tree hash for the eslint Node stack (Idea §5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedVersion(pub String);

impl PinnedVersion {
    /// Wraps a pinned version string.
    pub fn new(version: impl Into<String>) -> Self {
        Self(version.into())
    }
}

/// How a provider binary is sourced (Idea §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProviderSource {
    /// Pinned + auto-managed (default; the reproducible path).
    #[default]
    Pinned,
    /// The explicit `--system-tools` escape hatch — never an equal first-class mode.
    System,
}

impl ProviderSource {
    /// Whether the committed baseline may be updated under this source. Only
    /// **pinned** tools may update a baseline (Idea §5).
    #[must_use]
    pub const fn may_update_baseline(self) -> bool {
        matches!(self, ProviderSource::Pinned)
    }
}

/// The provider cache root (`$XDG_CACHE_HOME/commenter-cat/providers` or
/// `$HOME/.cache/commenter-cat/providers`), if determinable from the environment.
#[must_use]
pub fn provider_cache_dir(get_env: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let value = |key: &str| get_env(key).filter(|v| !v.is_empty());
    if let Some(xdg) = value("XDG_CACHE_HOME") {
        return Some(PathBuf::from(xdg).join("commenter-cat").join("providers"));
    }
    value("HOME").map(|home| {
        PathBuf::from(home)
            .join(".cache")
            .join("commenter-cat")
            .join("providers")
    })
}

/// The cache path for a specific pinned provider version.
#[must_use]
pub fn provider_path(cache_dir: &Path, provider: &str, version: &PinnedVersion) -> PathBuf {
    cache_dir.join(provider).join(&version.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |k: &str| map.get(k).cloned()
    }

    #[test]
    fn test_pinned_is_default_and_only_pinned_updates_baseline() {
        assert_eq!(ProviderSource::default(), ProviderSource::Pinned);
        assert!(ProviderSource::Pinned.may_update_baseline());
        assert!(!ProviderSource::System.may_update_baseline());
    }

    #[test]
    fn test_cache_dir_prefers_xdg_then_home() {
        let cache = provider_cache_dir(env(&[("XDG_CACHE_HOME", "/c")])).unwrap();
        assert!(cache.ends_with("commenter-cat/providers") && cache.starts_with("/c"));
        let home = provider_cache_dir(env(&[("HOME", "/home/u")])).unwrap();
        assert!(home.starts_with("/home/u/.cache/commenter-cat/providers"));
        assert!(provider_cache_dir(env(&[])).is_none());
    }

    #[test]
    fn test_provider_path_layout() {
        let cache = PathBuf::from("/c/commenter-cat/providers");
        let path = provider_path(&cache, "ruff", &PinnedVersion::new("0.14.2"));
        assert_eq!(
            path,
            PathBuf::from("/c/commenter-cat/providers/ruff/0.14.2")
        );
    }
}
