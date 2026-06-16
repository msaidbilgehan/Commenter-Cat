//! The committed baseline file format (Idea §5; task 7.5).
//!
//! `commenter-cat.baseline.toml` is a **committed** file beside the config —
//! *shared truth*, and therefore **outside** the gitignored `.commenter-cat/`
//! cache. It is sorted and line-oriented (one entry per suppressed identity),
//! canonically ordered to minimize merge conflicts (lockfile-style). Entries are
//! matched at **Tier 2** (cosmetic identity, never fuzzy, Idea §4/§5).

use serde::{Deserialize, Serialize};

/// The committed baseline filename (beside the config, outside the cache).
pub const BASELINE_FILENAME: &str = "commenter-cat.baseline.toml";

/// The current baseline format version (migrated in place, Idea §11).
pub const BASELINE_VERSION: u32 = 1;

/// One suppressed identity `(bound_symbol, cosmetic_fingerprint, rule)` with an
/// optional reason/date (Idea §5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaselineEntry {
    /// The bound symbol (`None` for an orphan comment).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub bound_symbol: Option<String>,
    /// The cosmetic fingerprint (Tier-2 identity).
    pub cosmetic_fingerprint: String,
    /// The suppressed rule (`provider_rule_id`, bare id, category, or origin).
    pub rule: String,
    /// Optional human reason.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reason: Option<String>,
    /// Optional ISO date the entry was accepted.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub date: Option<String>,
}

impl BaselineEntry {
    /// The canonical sort/identity key `(bound_symbol, fingerprint, rule)` —
    /// reason/date do not participate (Idea §5 canonical ordering).
    #[must_use]
    pub fn key(&self) -> (Option<&str>, &str, &str) {
        (
            self.bound_symbol.as_deref(),
            &self.cosmetic_fingerprint,
            &self.rule,
        )
    }
}

/// The committed baseline (Idea §5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Baseline {
    /// Format version.
    pub version: u32,
    /// Suppressed identities (canonically ordered on save).
    #[serde(default, rename = "entry")]
    pub entries: Vec<BaselineEntry>,
}

impl Default for Baseline {
    fn default() -> Self {
        Self {
            version: BASELINE_VERSION,
            entries: Vec::new(),
        }
    }
}

impl Baseline {
    /// Sorts entries into canonical order and removes duplicates (lockfile-style).
    pub fn canonicalize(&mut self) {
        self.entries.sort_by(|a, b| a.key().cmp(&b.key()));
        self.entries.dedup_by(|a, b| a.key() == b.key());
    }

    /// Whether the baseline suppresses a finding with this Tier-2 identity + rule.
    #[must_use]
    pub fn contains(&self, bound_symbol: Option<&str>, fingerprint: &str, rule: &str) -> bool {
        self.entries
            .iter()
            .any(|e| e.key() == (bound_symbol, fingerprint, rule))
    }
}
