//! `commenter-cat doctor` — version + config validation (Idea §5; task 6.7).
//!
//! The promise is "same source + same config ⇒ same findings". `commenter-cat doctor`
//! validates **both** the pinned provider version and the resolved-config
//! fingerprint against the committed baseline, so a silent version skew or
//! config drift is surfaced rather than allowed to make CI and a laptop disagree
//! (Idea §5). The full comparability key is `(commenter_cat_ruleset_version,
//! provider_version, config_hash)`.

use serde::{Deserialize, Serialize};

use commenter_cat_core::version::ComparabilityKey;

/// A provider's recorded state — its pinned version and resolved-config hash
/// (the baseline `[provider_state.<name>]`, Idea §5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderState {
    /// The provider id.
    pub provider: String,
    /// The pinned version (SemVer scalar or Node lockfile-tree hash).
    pub version: String,
    /// The resolved-effective-config hash (`sha256:…`).
    pub config_hash: String,
}

impl ProviderState {
    /// Builds a state.
    pub fn new(
        provider: impl Into<String>,
        version: impl Into<String>,
        config_hash: impl Into<String>,
    ) -> Self {
        Self {
            provider: provider.into(),
            version: version.into(),
            config_hash: config_hash.into(),
        }
    }

    /// The comparability key for this state (Idea §5), stamped with the current
    /// ruleset version.
    #[must_use]
    pub fn comparability_key(&self) -> ComparabilityKey {
        ComparabilityKey::current(&self.version, &self.config_hash)
    }
}

/// The doctor verdict for one provider: whether its version and config still
/// match the baseline (Idea §5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorReport {
    /// The provider id.
    pub provider: String,
    /// Whether the effective version matches the baseline.
    pub version_match: bool,
    /// Whether the resolved-config hash matches the baseline.
    pub config_match: bool,
    /// The baseline version (for display).
    pub baseline_version: String,
    /// The current effective version (for display).
    pub current_version: String,
}

impl DoctorReport {
    /// Whether findings are comparable to the baseline (both match).
    #[must_use]
    pub fn is_comparable(&self) -> bool {
        self.version_match && self.config_match
    }
}

/// Compares the baseline state to the current (effective, i.e. pinned) state.
///
/// Under pinned mode the *pinned* version is the effective one; a system version
/// is ignored (and shown separately by the renderer). A version skew or a
/// config-differs is reported as a non-comparable verdict.
#[must_use]
pub fn diagnose(baseline: &ProviderState, current: &ProviderState) -> DoctorReport {
    DoctorReport {
        provider: baseline.provider.clone(),
        version_match: baseline.comparability_key().provider_version
            == current.comparability_key().provider_version,
        config_match: baseline.config_hash == current.config_hash,
        baseline_version: baseline.version.clone(),
        current_version: current.version.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matching_state_is_comparable() {
        let state = ProviderState::new("ruff", "0.14.2", "sha256:abc");
        let report = diagnose(&state, &state);
        assert!(report.is_comparable());
        assert!(report.version_match && report.config_match);
    }

    #[test]
    fn test_version_skew_flagged() {
        let baseline = ProviderState::new("ruff", "0.14.2", "sha256:abc");
        let current = ProviderState::new("ruff", "0.15.0", "sha256:abc");
        let report = diagnose(&baseline, &current);
        assert!(!report.version_match);
        assert!(report.config_match);
        assert!(!report.is_comparable());
    }

    #[test]
    fn test_config_differs_flagged() {
        // Same pinned version, different resolved config → findings may change.
        let baseline = ProviderState::new("eslint", "10.3.0", "sha256:old");
        let current = ProviderState::new("eslint", "10.3.0", "sha256:new");
        let report = diagnose(&baseline, &current);
        assert!(report.version_match);
        assert!(!report.config_match);
        assert!(!report.is_comparable());
    }

    #[test]
    fn test_comparability_key_reflects_both() {
        let a = ProviderState::new("ruff", "0.14.2", "sha256:abc");
        let b = ProviderState::new("ruff", "0.14.2", "sha256:abc");
        assert_eq!(a.comparability_key(), b.comparability_key());
        let c = ProviderState::new("ruff", "0.14.2", "sha256:xyz");
        assert_ne!(a.comparability_key(), c.comparability_key());
    }
}
