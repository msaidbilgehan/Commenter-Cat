//! Canonical 4-level severity (Idea §5).
//!
//! Every provider's native scale is resolved into this one ladder so cross-
//! language findings are consistent. The variants are declared in **ascending**
//! order — `Info < Warning < Error < Critical` — because CI gating is a
//! threshold test: `fail_on = "error"` fails on `Error` and `Critical`
//! ([`Severity::fails_ci`]).
//!
//! This module owns the *type and ordering*. The category-anchored resolution
//! that maps a provider finding to a `Severity` (config override → category
//! default → per-tool table) is built in Phase 3 (`3.2`), on top of this enum.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Canonical severity level for a normalized finding (Idea §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Stylistic / advisory; never fails CI by default (e.g. `doc_style`).
    Info,
    /// Unexpected but non-blocking (e.g. `doc_missing`, `marker_stale`).
    Warning,
    /// A real defect; fails CI at the default `fail_on` (e.g. `doc_drift`).
    Error,
    /// Highest urgency; always fails CI (e.g. `secret`).
    Critical,
}

impl Severity {
    /// Every level, ascending. Useful for iteration and exhaustive tables.
    pub const ALL: [Severity; 4] = [
        Severity::Info,
        Severity::Warning,
        Severity::Error,
        Severity::Critical,
    ];

    /// The lowercase config/CLI token for this level (matches the serde form).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
            Severity::Critical => "critical",
        }
    }

    /// Parses a config/CLI token into a level, or `None` if unrecognized.
    #[must_use]
    pub fn from_token(token: &str) -> Option<Severity> {
        Severity::ALL.into_iter().find(|sev| sev.as_str() == token)
    }

    /// Whether a finding at this level should fail CI given the `fail_on`
    /// threshold (Idea §5: "fail the build on any finding at or above this
    /// level").
    #[must_use]
    pub fn fails_ci(self, fail_on: Severity) -> bool {
        self >= fail_on
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ascending_order() {
        assert!(Severity::Info < Severity::Warning);
        assert!(Severity::Warning < Severity::Error);
        assert!(Severity::Error < Severity::Critical);
    }

    #[test]
    fn test_fails_ci_threshold() {
        // Default threshold (Idea §12): fail_on = "error".
        assert!(Severity::Critical.fails_ci(Severity::Error));
        assert!(Severity::Error.fails_ci(Severity::Error));
        assert!(!Severity::Warning.fails_ci(Severity::Error));
        assert!(!Severity::Info.fails_ci(Severity::Error));
    }

    #[test]
    fn test_token_round_trips_for_all_variants() {
        for sev in Severity::ALL {
            assert_eq!(Severity::from_token(sev.as_str()), Some(sev));
            assert_eq!(sev.to_string(), sev.as_str());
        }
    }

    #[test]
    fn test_unknown_token_is_none() {
        assert_eq!(Severity::from_token("fatal"), None);
        assert_eq!(
            Severity::from_token("Error"),
            None,
            "tokens are case-sensitive"
        );
    }
}
