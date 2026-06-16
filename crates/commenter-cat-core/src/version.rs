//! Independently-versioned contract constants (Idea §11).
//!
//! Commenter-Cat carries **several** version contracts, and *conflating them is
//! the trap* (Idea §11). Each is a distinct integer with its own evolution rule:
//!
//! | Constant | Contract | Rule |
//! |---|---|---|
//! | [`SCHEMA_VERSION_JSONL`] | JSONL output stream | reader supports current + N−1; forward-only |
//! | [`INPUTS_DB_SCHEMA_VERSION`] | content-addressed `inputs.db` | versioned separately; rarely bumps; survives index bumps |
//! | [`INDEX_DB_SCHEMA_VERSION`] | derived `index.db` | **rebuilt** from `inputs.db`, never migrated |
//! | [`COMMENTER_CAT_RULESET_VERSION`] | canonical rule/category vocabulary | bump = a findings-comparability event (re-baseline), not a binary break |
//! | [`MANIFEST_VERSION`] | provider manifest format | Commenter-Cat reads current + prior major |
//! | [`CONFIG_VERSION`] | on-disk config schema | unknown future version = hard error |
//!
//! The [`ComparabilityKey`] bundles the triple that determines whether two
//! provider runs are comparable (Idea §5, §11): if it matches, findings are
//! reproducible; if it differs, a re-baseline is required.

use serde::{Deserialize, Serialize};

/// Schema version of the canonical JSONL output stream (Idea §4b, §11).
///
/// Readers support the current version plus N−1; the stream is forward-only.
pub const SCHEMA_VERSION_JSONL: u32 = 1;

/// Schema version of the content-addressed input store, `inputs.db` (Idea §6).
///
/// Keyed by content hash, so it survives `index.db` bumps without re-running
/// providers or re-embedding. Bumps rarely; a bump forces a true cold rebuild.
pub const INPUTS_DB_SCHEMA_VERSION: u32 = 1;

/// Schema version of the derived query index, `index.db` (Idea §6).
///
/// On a bump, `index.db` is **rebuilt** from `inputs.db` by the deterministic
/// native pass — never migrated in place (Idea §11, "rebuild over migrate").
pub const INDEX_DB_SCHEMA_VERSION: u32 = 1;

/// Version of Commenter-Cat's canonical rule/category vocabulary (Idea §5, §11).
///
/// A bump changes which rules Commenter-Cat ingests or how they map to canonical ids /
/// categories, which changes findings. It is therefore part of the
/// [`ComparabilityKey`] and a findings-comparability event — **not** a
/// binary-breaking change.
pub const COMMENTER_CAT_RULESET_VERSION: u32 = 1;

/// Version of the provider manifest format (Idea §5, §11).
///
/// Commenter-Cat reads the current and prior major so third-party adapters survive a Commenter-Cat
/// upgrade. Currently `1`.
pub const MANIFEST_VERSION: u32 = 1;

/// Version of the on-disk TOML configuration schema (Idea §11, §12).
///
/// A config declaring an unknown *future* version is a hard error with
/// guidance; deprecations are warned at least one MINOR before removal.
/// Currently `1`.
pub const CONFIG_VERSION: u32 = 1;

/// The `commenter-cat` binary's SemVer, sourced from the crate manifest (Idea §11: 1.0+).
#[must_use]
pub fn commenter_cat_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Whether a JSONL stream tagged `version` can be read by this build.
///
/// Per Idea §11, readers support the current schema and the one before it
/// (`current` and `current − 1`); newer streams are unreadable (forward-only).
#[must_use]
pub fn jsonl_schema_is_supported(version: u32) -> bool {
    version <= SCHEMA_VERSION_JSONL && version + 1 >= SCHEMA_VERSION_JSONL
}

/// Whether a provider manifest declaring `manifest_version` is loadable.
///
/// Per Idea §11, Commenter-Cat reads the current manifest major and the prior one, so an
/// adapter written for an older Commenter-Cat keeps working after an upgrade.
#[must_use]
pub fn manifest_version_is_supported(version: u32) -> bool {
    version <= MANIFEST_VERSION && version + 1 >= MANIFEST_VERSION
}

/// The triple that decides whether two provider runs are comparable (Idea §5).
///
/// Reproducibility is "same source + same config ⇒ same findings". Two runs are
/// comparable only when **all three** of these agree: a Commenter-Cat ruleset change, a
/// provider upgrade, or a resolved-config change each independently invalidates
/// comparability and requires a re-baseline (surfaced by `commenter-cat doctor`).
///
/// `provider_version` is a string because single-binary providers resolve to a
/// SemVer (`"0.14.2"`) while the eslint Node stack resolves to a locked
/// dependency *tree*, represented by its lockfile hash (Idea §5).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ComparabilityKey {
    /// Commenter-Cat's canonical-vocabulary version at the time of the run.
    pub commenter_cat_ruleset_version: u32,
    /// The provider's pinned version (SemVer scalar or Node lockfile-tree hash).
    pub provider_version: String,
    /// Hash of the provider's *resolved effective config* (e.g. `"sha256:…"`),
    /// taken from the tool's own dump so cascaded `extends` are captured.
    pub config_hash: String,
}

impl ComparabilityKey {
    /// Builds a key from a provider version and resolved-config hash, stamping
    /// it with the current [`COMMENTER_CAT_RULESET_VERSION`].
    pub fn current(provider_version: impl Into<String>, config_hash: impl Into<String>) -> Self {
        Self {
            commenter_cat_ruleset_version: COMMENTER_CAT_RULESET_VERSION,
            provider_version: provider_version.into(),
            config_hash: config_hash.into(),
        }
    }

    /// Builds a key with an explicit ruleset version (for reading historical
    /// baseline artifacts produced by a different Commenter-Cat build).
    pub fn new(
        commenter_cat_ruleset_version: u32,
        provider_version: impl Into<String>,
        config_hash: impl Into<String>,
    ) -> Self {
        Self {
            commenter_cat_ruleset_version,
            provider_version: provider_version.into(),
            config_hash: config_hash.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_comparability_key_equality() {
        let a = ComparabilityKey::current("0.14.2", "sha256:abc");
        let b = ComparabilityKey::current("0.14.2", "sha256:abc");
        assert_eq!(a, b, "identical inputs must produce equal keys");

        // Any one component differing breaks comparability (re-baseline event).
        let diff_version = ComparabilityKey::current("0.14.3", "sha256:abc");
        let diff_config = ComparabilityKey::current("0.14.2", "sha256:def");
        let diff_ruleset =
            ComparabilityKey::new(COMMENTER_CAT_RULESET_VERSION + 1, "0.14.2", "sha256:abc");
        assert_ne!(a, diff_version);
        assert_ne!(a, diff_config);
        assert_ne!(a, diff_ruleset);
    }

    #[test]
    fn test_comparability_key_hashes_consistently() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(ComparabilityKey::current("0.14.2", "sha256:abc"));
        assert!(set.contains(&ComparabilityKey::current("0.14.2", "sha256:abc")));
        assert!(!set.contains(&ComparabilityKey::current("0.14.2", "sha256:xyz")));
    }

    #[test]
    fn test_current_stamps_ruleset_version() {
        let key = ComparabilityKey::current("1.0.0", "sha256:0");
        assert_eq!(
            key.commenter_cat_ruleset_version,
            COMMENTER_CAT_RULESET_VERSION
        );
    }

    #[test]
    fn test_schema_support_windows() {
        // Current is always supported; current − 1 is supported; future is not.
        assert!(jsonl_schema_is_supported(SCHEMA_VERSION_JSONL));
        assert!(!jsonl_schema_is_supported(SCHEMA_VERSION_JSONL + 1));
        assert!(manifest_version_is_supported(MANIFEST_VERSION));
        assert!(!manifest_version_is_supported(MANIFEST_VERSION + 1));
    }

    #[test]
    fn test_commenter_cat_version_is_nonempty() {
        assert!(!commenter_cat_version().is_empty());
    }
}
