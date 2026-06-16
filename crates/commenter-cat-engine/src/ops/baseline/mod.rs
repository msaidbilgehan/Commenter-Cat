//! The committed baseline (Idea §5; task 7.5).
//!
//! Inline directives (task 7.4) and the baseline are two inputs to **one**
//! suppression pass. `commenter-cat baseline accept` snapshots current findings; `commenter-cat
//! baseline prune` drops entries whose findings no longer occur. Matching is
//! **Tier 2** (cosmetic identity), never fuzzy — a false match there would hide
//! a real finding (Idea §4, §5).

pub mod file_format;

use std::collections::BTreeSet;
use std::path::Path;

use commenter_cat_core::error::{CommenterCatError, CommenterCatResult};

pub use file_format::{Baseline, BaselineEntry, BASELINE_FILENAME, BASELINE_VERSION};

/// A `(bound_symbol, cosmetic_fingerprint, rule)` identity to baseline — the
/// Tier-2 key a finding is matched on (Idea §5).
pub type SuppressedIdentity = (Option<String>, String, String);

/// Snapshots a set of suppressed identities into a canonical baseline
/// (`commenter-cat baseline accept`). Deterministic: sorted + de-duplicated.
#[must_use]
pub fn accept(identities: &[SuppressedIdentity]) -> Baseline {
    let entries = identities
        .iter()
        .map(|(bound_symbol, fingerprint, rule)| BaselineEntry {
            bound_symbol: bound_symbol.clone(),
            cosmetic_fingerprint: fingerprint.clone(),
            rule: rule.clone(),
            reason: None,
            date: None,
        })
        .collect();
    let mut baseline = Baseline {
        version: BASELINE_VERSION,
        entries,
    };
    baseline.canonicalize();
    baseline
}

/// Drops baseline entries whose identity no longer occurs in `current`
/// (`commenter-cat baseline prune`). Reasons/dates on surviving entries are preserved.
#[must_use]
pub fn prune(baseline: &Baseline, current: &[SuppressedIdentity]) -> Baseline {
    let live: BTreeSet<(Option<&str>, &str, &str)> = current
        .iter()
        .map(|(symbol, fingerprint, rule)| (symbol.as_deref(), fingerprint.as_str(), rule.as_str()))
        .collect();
    let entries = baseline
        .entries
        .iter()
        .filter(|e| live.contains(&e.key()))
        .cloned()
        .collect();
    let mut pruned = Baseline {
        version: baseline.version,
        entries,
    };
    pruned.canonicalize();
    pruned
}

/// Loads and validates a baseline file.
///
/// # Errors
/// Returns [`CommenterCatError::Config`] if the file cannot be read, is invalid TOML, or
/// declares an unknown future version.
pub fn load(path: &Path) -> CommenterCatResult<Baseline> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        CommenterCatError::config(format!("reading baseline {}", path.display())).caused_by(e)
    })?;
    let baseline: Baseline = toml::from_str(&text).map_err(|e| {
        CommenterCatError::config(format!("parsing baseline {}", path.display())).caused_by(e)
    })?;
    if baseline.version > BASELINE_VERSION {
        return Err(CommenterCatError::config(format!(
            "baseline at {} declares version {}, but this commenter-cat supports up to {BASELINE_VERSION}; \
             run `commenter-cat baseline migrate`",
            path.display(),
            baseline.version
        )));
    }
    Ok(baseline)
}

/// Writes a baseline in canonical order.
///
/// # Errors
/// Returns [`CommenterCatError::Config`] on serialization or write failure.
pub fn save(baseline: &Baseline, path: &Path) -> CommenterCatResult<()> {
    let mut canonical = baseline.clone();
    canonical.canonicalize();
    let text = toml::to_string(&canonical)
        .map_err(|e| CommenterCatError::config("serializing baseline").caused_by(e))?;
    std::fs::write(path, text).map_err(|e| {
        CommenterCatError::config(format!("writing baseline {}", path.display())).caused_by(e)
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(symbol: &str, fingerprint: &str, rule: &str) -> SuppressedIdentity {
        (
            Some(symbol.to_owned()),
            fingerprint.to_owned(),
            rule.to_owned(),
        )
    }

    #[test]
    fn test_accept_is_deterministic_and_canonical() {
        let items = vec![
            id("z.f", "fp2", "D100"),
            id("a.f", "fp1", "D417"),
            id("a.f", "fp1", "D417"),
        ];
        let baseline = accept(&items);
        // De-duplicated and sorted by (bound_symbol, fingerprint, rule).
        assert_eq!(baseline.entries.len(), 2);
        assert_eq!(baseline.entries[0].bound_symbol.as_deref(), Some("a.f"));
        assert_eq!(baseline.entries[1].bound_symbol.as_deref(), Some("z.f"));
        // Re-accepting the same set yields an identical baseline.
        assert_eq!(accept(&items), baseline);
    }

    #[test]
    fn test_tier2_contains() {
        let baseline = accept(&[id("app.main", "abc", "ruff:D417")]);
        assert!(baseline.contains(Some("app.main"), "abc", "ruff:D417"));
        // A different fingerprint (reworded comment) does NOT match (Tier 2 only).
        assert!(!baseline.contains(Some("app.main"), "xyz", "ruff:D417"));
        assert!(!baseline.contains(Some("other"), "abc", "ruff:D417"));
    }

    #[test]
    fn test_prune_removes_stale_entries() {
        let baseline = accept(&[id("a.f", "fp1", "D417"), id("b.g", "fp2", "D100")]);
        // Only a.f/fp1/D417 still occurs.
        let pruned = prune(&baseline, &[id("a.f", "fp1", "D417")]);
        assert_eq!(pruned.entries.len(), 1);
        assert_eq!(pruned.entries[0].bound_symbol.as_deref(), Some("a.f"));
    }

    #[test]
    fn test_save_load_round_trip_canonical() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join(BASELINE_FILENAME);
        let baseline = accept(&[id("a.f", "fp1", "D417")]);
        save(&baseline, &path).unwrap();
        assert_eq!(load(&path).unwrap(), baseline);
    }

    #[test]
    fn test_unknown_future_version_errors() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join(BASELINE_FILENAME);
        std::fs::write(&path, "version = 99\n").unwrap();
        assert!(load(&path).is_err());
    }
}
