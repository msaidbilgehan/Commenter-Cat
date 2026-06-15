//! The committed issue-ledger file format (Idea §9).
//!
//! `comment-finder.issues.toml` is the idempotency ledger for comment-to-issue
//! filing — each comment's **Tier-4 identity token** mapped to the tracker issue
//! it was filed against. It is **committed** beside the baseline, *not* in the
//! gitignored cache: idempotency must be **shared truth** — a developer who files
//! an issue and the CI run that later syncs must read the *same* ledger, or every
//! run re-files. (The §9 note hinted at "the index"; a shared-identity ledger
//! belongs in committed truth, like the baseline, for exactly that reason.)

use std::path::Path;

use serde::{Deserialize, Serialize};

use cf_core::error::{CfError, CfResult};

use super::{IssueLedger, IssueRef};

/// The committed ledger filename (beside the config, outside the cache).
pub const LEDGER_FILENAME: &str = "comment-finder.issues.toml";

/// The current ledger format version (migrated in place, Idea §11).
pub const LEDGER_VERSION: u32 = 1;

/// One filed identity: its Tier-4 token and the issue it maps to.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LedgerEntry {
    /// The Tier-4 identity token (`symbol|kind|marker`).
    token: String,
    /// The tracker issue id.
    id: String,
    /// The canonical issue URL.
    url: String,
    /// Whether the issue is closed (last observed; drives sync).
    #[serde(default)]
    closed: bool,
}

/// The committed ledger file (Idea §9).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LedgerFile {
    /// Format version.
    version: u32,
    /// Filed identities (canonically token-sorted on save).
    #[serde(default, rename = "issue")]
    entries: Vec<LedgerEntry>,
}

/// Loads and validates the issue ledger, hydrating an [`IssueLedger`].
///
/// # Errors
/// Returns [`CfError::Config`] if the file cannot be read, is invalid TOML, or
/// declares an unknown future version.
pub fn load(path: &Path) -> CfResult<IssueLedger> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        CfError::config(format!("reading issue ledger {}", path.display())).caused_by(e)
    })?;
    let file: LedgerFile = toml::from_str(&text).map_err(|e| {
        CfError::config(format!("parsing issue ledger {}", path.display())).caused_by(e)
    })?;
    if file.version > LEDGER_VERSION {
        return Err(CfError::config(format!(
            "issue ledger at {} declares version {}, but this cf supports up to {LEDGER_VERSION}",
            path.display(),
            file.version
        )));
    }
    let mut ledger = IssueLedger::new();
    for entry in file.entries {
        ledger.record(
            entry.token,
            IssueRef {
                id: entry.id,
                url: entry.url,
                closed: entry.closed,
            },
        );
    }
    Ok(ledger)
}

/// Writes the issue ledger in canonical (token-sorted) order.
///
/// # Errors
/// Returns [`CfError::Config`] on serialization or write failure.
pub fn save(ledger: &IssueLedger, path: &Path) -> CfResult<()> {
    let mut entries: Vec<LedgerEntry> = ledger
        .iter()
        .map(|(token, issue)| LedgerEntry {
            token: token.to_owned(),
            id: issue.id.clone(),
            url: issue.url.clone(),
            closed: issue.closed,
        })
        .collect();
    entries.sort_by(|a, b| a.token.cmp(&b.token));
    let file = LedgerFile {
        version: LEDGER_VERSION,
        entries,
    };
    let text = toml::to_string(&file)
        .map_err(|e| CfError::config("serializing issue ledger").caused_by(e))?;
    std::fs::write(path, text).map_err(|e| {
        CfError::config(format!("writing issue ledger {}", path.display())).caused_by(e)
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ledger_round_trips_through_toml() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join(LEDGER_FILENAME);
        let mut ledger = IssueLedger::new();
        ledger.record(
            "app.f|line|TODO".to_owned(),
            IssueRef {
                id: "12".to_owned(),
                url: "https://x/12".to_owned(),
                closed: false,
            },
        );
        ledger.record(
            "app.g|line|FIXME".to_owned(),
            IssueRef {
                id: "13".to_owned(),
                url: "https://x/13".to_owned(),
                closed: true,
            },
        );
        save(&ledger, &path).unwrap();

        let loaded = load(&path).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded.get("app.f|line|TODO").unwrap().id, "12");
        assert!(loaded.get("app.g|line|FIXME").unwrap().closed);
    }

    #[test]
    fn test_future_version_is_rejected() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join(LEDGER_FILENAME);
        std::fs::write(&path, "version = 999\n").unwrap();
        assert!(load(&path).is_err());
    }
}
