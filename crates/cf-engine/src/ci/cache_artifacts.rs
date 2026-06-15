//! Two-layer CI cache keying (Idea §6, §7).
//!
//! The whole point: a CF upgrade should **reuse expensive provider results**. So
//! the two artifacts are keyed differently:
//!
//! * `inputs.db` — keyed on provider versions + config hashes + the inputs
//!   schema. It **excludes** `cf_ruleset_version`, so bumping CF does not
//!   invalidate it: the provider outputs are reused.
//! * `index.db` — keyed on the *full* comparability key (inputs material **plus**
//!   `cf_ruleset_version` + the index schema). A CF upgrade misses, and the index
//!   is **re-derived from `inputs.db`** — never cold-scanned.

use cf_core::version::{CF_RULESET_VERSION, INDEX_DB_SCHEMA_VERSION, INPUTS_DB_SCHEMA_VERSION};

use crate::hash;

/// A provider's identity for cache keying — its pinned version + resolved config
/// hash (Idea §6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderFingerprint {
    /// The provider id (e.g. `"ruff"`).
    pub id: String,
    /// The pinned version (SemVer scalar or lockfile-tree hash).
    pub version: String,
    /// Hash of the resolved effective config.
    pub config_hash: String,
}

impl ProviderFingerprint {
    /// Builds a fingerprint.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        version: impl Into<String>,
        config_hash: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            version: version.into(),
            config_hash: config_hash.into(),
        }
    }
}

/// The deterministic, provider-id-sorted material both keys derive from.
fn provider_material(providers: &[ProviderFingerprint]) -> String {
    let mut sorted: Vec<&ProviderFingerprint> = providers.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    let mut material = format!("inputs_schema={INPUTS_DB_SCHEMA_VERSION}");
    for provider in sorted {
        material.push_str(&format!(
            ";{}={}@{}",
            provider.id, provider.version, provider.config_hash
        ));
    }
    material
}

/// The `inputs.db` cache key — provider versions + config hashes + inputs schema,
/// **without** `cf_ruleset_version`, so a CF upgrade reuses provider results.
#[must_use]
pub fn inputs_cache_key(providers: &[ProviderFingerprint]) -> String {
    hash::sha256_hex(provider_material(providers).as_bytes())
}

/// The `index.db` cache key — the full comparability key (inputs material plus
/// `cf_ruleset_version` + index schema), so a CF upgrade misses and the index is
/// re-derived from `inputs.db`.
#[must_use]
pub fn index_cache_key(providers: &[ProviderFingerprint], cf_ruleset_version: u32) -> String {
    let material = format!(
        "{};cf_ruleset={cf_ruleset_version};index_schema={INDEX_DB_SCHEMA_VERSION}",
        provider_material(providers)
    );
    hash::sha256_hex(material.as_bytes())
}

/// How a CI run will restore its caches (Idea §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestorePlan {
    /// `index.db` hit — use it directly, no work.
    UseIndex,
    /// `index.db` miss but `inputs.db` hit — re-derive the index from inputs
    /// (no provider re-runs, no cold scan). The CF-upgrade fast path.
    ReDeriveFromInputs,
    /// `inputs.db` miss — re-run providers from scratch (cold).
    ColdScan,
}

/// Chooses the restore plan from the two cache hits (Idea §7).
#[must_use]
pub fn plan_restore(inputs_hit: bool, index_hit: bool) -> RestorePlan {
    match (inputs_hit, index_hit) {
        (_, true) => RestorePlan::UseIndex,
        (true, false) => RestorePlan::ReDeriveFromInputs,
        (false, false) => RestorePlan::ColdScan,
    }
}

/// The current CF ruleset version (for the index key at run time).
#[must_use]
pub fn current_ruleset_version() -> u32 {
    CF_RULESET_VERSION
}

#[cfg(test)]
mod tests {
    use super::*;

    fn providers() -> Vec<ProviderFingerprint> {
        vec![
            ProviderFingerprint::new("ruff", "0.14.2", "sha256:aaa"),
            ProviderFingerprint::new("eslint", "9.1.0", "sha256:bbb"),
        ]
    }

    #[test]
    fn test_cf_upgrade_reuses_inputs_but_remisses_index() {
        let providers = providers();
        let v = current_ruleset_version();

        // A CF upgrade = a bumped ruleset version, same providers.
        let inputs_before = inputs_cache_key(&providers);
        let inputs_after = inputs_cache_key(&providers);
        assert_eq!(
            inputs_before, inputs_after,
            "inputs key ignores the CF version"
        );

        let index_before = index_cache_key(&providers, v);
        let index_after = index_cache_key(&providers, v + 1);
        assert_ne!(index_before, index_after, "index key tracks the CF version");
    }

    #[test]
    fn test_provider_change_invalidates_inputs() {
        let key = inputs_cache_key(&providers());
        let bumped = vec![
            ProviderFingerprint::new("ruff", "0.15.0", "sha256:aaa"), // new version
            ProviderFingerprint::new("eslint", "9.1.0", "sha256:bbb"),
        ];
        assert_ne!(key, inputs_cache_key(&bumped));
    }

    #[test]
    fn test_key_is_order_independent() {
        let mut reordered = providers();
        reordered.reverse();
        assert_eq!(inputs_cache_key(&providers()), inputs_cache_key(&reordered));
    }

    #[test]
    fn test_restore_plan() {
        assert_eq!(plan_restore(true, true), RestorePlan::UseIndex);
        // The CF-upgrade fast path: inputs hit, index miss → re-derive.
        assert_eq!(plan_restore(true, false), RestorePlan::ReDeriveFromInputs);
        assert_eq!(plan_restore(false, false), RestorePlan::ColdScan);
    }
}
