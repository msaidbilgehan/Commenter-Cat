//! Node-runtime tiers for the eslint provider (Idea §5; task 6.6).
//!
//! ESLint drift comes from eslint/parser/plugin/TS versions, **not** the Node
//! patch version — so a Python-only repo is never made to download Node. Two
//! tiers, and every run records its [`ReproducibilityLevel`] so a degraded
//! guarantee is visible in CI, not silent (Idea §5).

use serde::{Deserialize, Serialize};

/// The reproducibility guarantee recorded in run metadata (Idea §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReproducibilityLevel {
    /// System Node + a pinned eslint/plugin tree (default).
    SemiHermetic,
    /// Fetched + pinned Node *and* the pinned tree. Single-binary providers
    /// (ruff/shellcheck/gitleaks) are `HERMETIC` already (no Node).
    Hermetic,
}

impl ReproducibilityLevel {
    /// The metadata token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ReproducibilityLevel::SemiHermetic => "SEMI_HERMETIC",
            ReproducibilityLevel::Hermetic => "HERMETIC",
        }
    }
}

/// The Node runtime tier for the eslint stack (Idea §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NodeRuntime {
    /// `default` — use system Node (≥ min supported) with a pinned plugin tree.
    #[default]
    SemiHermetic,
    /// `--hermetic` / `runtime = "hermetic"` — fetch and pin Node into the cache.
    Hermetic,
}

impl NodeRuntime {
    /// The reproducibility level this tier guarantees (Idea §5).
    #[must_use]
    pub const fn reproducibility_level(self) -> ReproducibilityLevel {
        match self {
            NodeRuntime::SemiHermetic => ReproducibilityLevel::SemiHermetic,
            NodeRuntime::Hermetic => ReproducibilityLevel::Hermetic,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tier_reports_reproducibility_level() {
        assert_eq!(
            NodeRuntime::SemiHermetic.reproducibility_level(),
            ReproducibilityLevel::SemiHermetic
        );
        assert_eq!(
            NodeRuntime::Hermetic.reproducibility_level(),
            ReproducibilityLevel::Hermetic
        );
        assert_eq!(NodeRuntime::default(), NodeRuntime::SemiHermetic);
    }

    #[test]
    fn test_reproducibility_level_tokens() {
        assert_eq!(ReproducibilityLevel::SemiHermetic.as_str(), "SEMI_HERMETIC");
        assert_eq!(ReproducibilityLevel::Hermetic.as_str(), "HERMETIC");
    }
}
