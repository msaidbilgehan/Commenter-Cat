//! The shared provider-contract harness (Idea §5, §11).
//!
//! Every adapter — manifest, native, and the dogfooded built-ins — must honor the
//! same contract: a declared coordinate system, a stable id, and run-state
//! semantics where a **failure is PARTIAL (findings unavailable), never EMPTY
//! (zero findings)**. This harness runs the contract over all built-ins so a new
//! adapter cannot quietly violate it. (Live subprocess runs, including the
//! provider-absent → SKIPPED path, are exercised by the integration suite.)

// Test code: unwrap/expect on known-good fixtures is idiomatic.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeSet;

use cf_core::finding::CoordinateSystem;
use cf_engine::provider::{builtins, ProviderRun, RuleProvider, RunState};

fn built_ins() -> Vec<Box<dyn RuleProvider>> {
    builtins::load_all()
        .expect("built-in manifests parse")
        .into_iter()
        .map(|provider| Box::new(provider) as Box<dyn RuleProvider>)
        .collect()
}

#[test]
fn every_builtin_declares_id_and_coordinate_system() {
    let providers = built_ins();
    assert!(!providers.is_empty(), "there are dogfooded built-ins");

    let mut ids = BTreeSet::new();
    for provider in &providers {
        assert!(!provider.id().is_empty(), "a provider id is non-empty");
        assert!(
            ids.insert(provider.id().to_owned()),
            "provider ids are unique"
        );

        // The coordinate convention is one of the known dialects (R6) — declared,
        // so columns are converted in exactly one place.
        let system = provider.capabilities().coordinate_system;
        assert!(
            matches!(
                system,
                CoordinateSystem::ZeroBasedUtf8
                    | CoordinateSystem::OneBasedUtf8
                    | CoordinateSystem::OneBasedUtf16
                    | CoordinateSystem::OneBasedChar
            ),
            "{} declares a known coordinate system",
            provider.id()
        );
    }
}

#[test]
fn failure_is_partial_not_zero() {
    // The load-bearing run-state contract (Idea §5): a crash/timeout/malformed
    // output is PARTIAL with findings *unavailable* — never silently EMPTY.
    let partial = ProviderRun::partial();
    assert_eq!(partial.state, RunState::Partial);
    assert!(
        partial.findings.is_empty(),
        "PARTIAL carries no findings — they are unavailable"
    );

    // Ran-with-nothing is EMPTY (genuinely zero), not PARTIAL.
    assert_eq!(ProviderRun::ran(vec![]).state, RunState::Empty);

    // Intentionally not executed is SKIPPED.
    assert_eq!(ProviderRun::skipped().state, RunState::Skipped);
}

#[test]
fn run_states_round_trip_their_tokens() {
    // The four states are the canonical trust signal (Idea §5), stable on disk.
    for (state, token) in [
        (RunState::Success, "SUCCESS"),
        (RunState::Empty, "EMPTY"),
        (RunState::Partial, "PARTIAL"),
        (RunState::Skipped, "SKIPPED"),
    ] {
        assert_eq!(state.as_str(), token);
        assert_eq!(RunState::from_token(token), Some(state));
    }
}
