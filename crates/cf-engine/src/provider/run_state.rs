//! The provider run-state machine (Idea §5; task 6.2).
//!
//! **Provider failure ≠ zero findings.** Each run resolves to exactly one state,
//! and the **exit code is not the signal** — linters exit nonzero merely on
//! findings, so *parsed JSON = ran* (Idea §5). A `PARTIAL` provider degrades the
//! diff confidence rather than masquerading as `EMPTY` (Idea §7).

use serde::{Deserialize, Serialize};

use cf_core::finding::Finding;

/// The result state of one provider invocation (Idea §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum RunState {
    /// Ran, findings available.
    Success,
    /// Ran, findings = 0.
    Empty,
    /// Failed (crash / timeout / malformed JSON) — findings unavailable, not zero.
    Partial,
    /// Intentionally not executed (provider absent, language off).
    Skipped,
}

impl RunState {
    /// The uppercase token (matches the serde form and `inputs.db` storage).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            RunState::Success => "SUCCESS",
            RunState::Empty => "EMPTY",
            RunState::Partial => "PARTIAL",
            RunState::Skipped => "SKIPPED",
        }
    }

    /// Parses a token, or `None` if unrecognized.
    #[must_use]
    pub fn from_token(token: &str) -> Option<RunState> {
        match token {
            "SUCCESS" => Some(RunState::Success),
            "EMPTY" => Some(RunState::Empty),
            "PARTIAL" => Some(RunState::Partial),
            "SKIPPED" => Some(RunState::Skipped),
            _ => None,
        }
    }

    /// Classifies a provider that **ran**, from its parsed-JSON result. The exit
    /// code is irrelevant — a successful parse means it ran (Idea §5).
    #[must_use]
    pub fn from_parsed(parsed: Result<&[Finding], &str>) -> RunState {
        match parsed {
            Ok([]) => RunState::Empty,
            Ok(_) => RunState::Success,
            Err(_) => RunState::Partial,
        }
    }

    /// Whether findings are **unavailable** (vs genuinely zero).
    #[must_use]
    pub const fn findings_unavailable(self) -> bool {
        matches!(self, RunState::Partial)
    }

    /// Whether `--strict` makes this state fatal (Idea §5: `--strict` makes
    /// `PARTIAL` fatal; the default `on_error = warn`).
    #[must_use]
    pub const fn is_fatal(self, strict: bool) -> bool {
        strict && matches!(self, RunState::Partial)
    }
}

/// Aggregates per-provider states into the run's `baseline_state` (Idea §5, §7):
/// any `PARTIAL` ⇒ `PARTIAL` (degraded diff confidence); else all `SKIPPED` ⇒
/// `SKIPPED`; else any `SUCCESS` ⇒ `SUCCESS`; else `EMPTY`.
#[must_use]
pub fn baseline_state(states: &[RunState]) -> RunState {
    if states.iter().any(|s| matches!(s, RunState::Partial)) {
        RunState::Partial
    } else if !states.is_empty() && states.iter().all(|s| matches!(s, RunState::Skipped)) {
        RunState::Skipped
    } else if states.iter().any(|s| matches!(s, RunState::Success)) {
        RunState::Success
    } else {
        RunState::Empty
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cf_core::finding::{Category, FindingTarget, Fix, Origin, Range};
    use cf_core::severity::Severity;
    use cf_core::symbol::CommentId;

    fn finding() -> Finding {
        Finding {
            file: "a.py".to_owned(),
            target: FindingTarget::Comment(CommentId::new("c")),
            range: Range::new(0, 1, 1, 1),
            origin: Origin::Ruff,
            provider_rule_id: "ruff:D100".to_owned(),
            canonical_rule_id: "D100".to_owned(),
            category: Category::DocMissing,
            severity: Severity::Warning,
            severity_native: None,
            message: "m".to_owned(),
            fix: Fix::None,
            url: None,
            also_from: Default::default(),
        }
    }

    #[test]
    fn test_malformed_json_is_partial_not_empty() {
        assert_eq!(
            RunState::from_parsed(Err("unexpected EOF")),
            RunState::Partial
        );
    }

    #[test]
    fn test_valid_json_with_findings_is_success_regardless_of_exit_code() {
        // The exit code is not modeled — a parsed result is what matters.
        let findings = [finding()];
        assert_eq!(RunState::from_parsed(Ok(&findings)), RunState::Success);
    }

    #[test]
    fn test_empty_findings_is_empty() {
        assert_eq!(RunState::from_parsed(Ok(&[])), RunState::Empty);
    }

    #[test]
    fn test_partial_is_fatal_only_under_strict() {
        assert!(RunState::Partial.is_fatal(true));
        assert!(!RunState::Partial.is_fatal(false));
        assert!(!RunState::Success.is_fatal(true));
        assert!(RunState::Partial.findings_unavailable());
        assert!(!RunState::Empty.findings_unavailable());
    }

    #[test]
    fn test_baseline_state_aggregation() {
        assert_eq!(
            baseline_state(&[RunState::Success, RunState::Partial]),
            RunState::Partial
        );
        assert_eq!(
            baseline_state(&[RunState::Skipped, RunState::Skipped]),
            RunState::Skipped
        );
        assert_eq!(
            baseline_state(&[RunState::Empty, RunState::Success]),
            RunState::Success
        );
        assert_eq!(
            baseline_state(&[RunState::Empty, RunState::Skipped]),
            RunState::Empty
        );
    }

    #[test]
    fn test_token_round_trip() {
        for state in [
            RunState::Success,
            RunState::Empty,
            RunState::Partial,
            RunState::Skipped,
        ] {
            assert_eq!(RunState::from_token(state.as_str()), Some(state));
        }
    }
}
