//! End-to-end integration tests on real fixture repositories (Idea §11).
//!
//! Real tree-sitter, real files, real git — mock only at seams. Exercises the
//! `cf check` fusion, filter-up suppression, the committed baseline diff, and the
//! **graceful-degradation path**: an absent provider degrades to SKIPPED (findings
//! intentionally not produced), never a crash or a false EMPTY.

// Test code: unwrap/expect on known-good fixtures is idiomatic.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use cf_core::config::ResolvedConfig;
use cf_core::finding::{Finding, Origin};
use cf_core::identity::fingerprint::cosmetic_fingerprint;
use cf_engine::ci::diff::new_findings;
use cf_engine::ops::baseline;
use cf_engine::ops::check::check;
use cf_engine::ops::suppress::{self, directives};
use cf_engine::provider::manifest::ManifestProvider;
use cf_engine::provider::{ProviderContext, RuleProvider, RunState};

/// Builds a throwaway git repo seeded with the given files.
fn repo_with(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::TempDir::new().unwrap();
    for (path, contents) in files {
        let full = dir.path().join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(full, contents).unwrap();
    }
    Command::new("git")
        .arg("init")
        .arg("-q")
        .current_dir(dir.path())
        .status()
        .unwrap();
    dir
}

/// The native-only provider set (no external tools).
const NO_PROVIDERS: [&dyn RuleProvider; 0] = [];

#[test]
fn check_produces_unified_findings_across_a_real_tree() {
    let dir = repo_with(&[(
        "pkg/app.py",
        "# TODO wire this up\ndef parse(data):\n    \"\"\"Parse it.\"\"\"\n    return data\n",
    )]);

    let result = check(dir.path(), &ResolvedConfig::default(), &NO_PROVIDERS).unwrap();

    // The TODO marker surfaces as a native finding bound to its comment.
    let findings: Vec<&Finding> = result.comments.iter().flat_map(|c| &c.findings).collect();
    assert!(
        findings
            .iter()
            .any(|f| f.origin == Origin::Native && f.message.contains("TODO")),
        "the native marker triage fused into the unified result"
    );
}

#[test]
fn directive_suppresses_a_matching_finding() {
    let dir = repo_with(&[("a.py", "# TODO debt here\nx = 1\n")]);
    let result = check(dir.path(), &ResolvedConfig::default(), &NO_PROVIDERS).unwrap();
    let findings: Vec<Finding> = result
        .comments
        .iter()
        .flat_map(|c| c.findings.iter().cloned())
        .collect();

    let todo = findings
        .iter()
        .find(|f| f.message.contains("TODO"))
        .expect("a TODO finding");
    // An inline directive on the finding's line, targeting the marker rule.
    let directive =
        directives::parse("# cf:disable-line=marker:TODO", todo.range.start_line).unwrap();

    let outcome = suppress::suppress(&findings, &[directive]);
    assert_eq!(
        outcome.decisions.len(),
        1,
        "the TODO finding is suppressed (flagged, not dropped)"
    );
    assert!(outcome.unused_directives.is_empty());
}

#[test]
fn baseline_excludes_known_findings_from_the_diff() {
    let dir = repo_with(&[("a.py", "# TODO old debt\nx = 1\n")]);
    let mut result = check(dir.path(), &ResolvedConfig::default(), &NO_PROVIDERS).unwrap();

    // Identity (the cosmetic fingerprint) is computed alongside the diff.
    for comment in &mut result.comments {
        comment.cosmetic_fingerprint = Some(cosmetic_fingerprint(&comment.raw_text));
    }
    // Baseline every current finding …
    let entries: Vec<(Option<String>, String, String)> = result
        .comments
        .iter()
        .flat_map(|c| {
            let symbol = c.bound_symbol.as_ref().map(|s| s.as_str().to_owned());
            let fingerprint = c.cosmetic_fingerprint.clone().unwrap_or_default();
            c.findings.iter().map(move |f| {
                (
                    symbol.clone(),
                    fingerprint.clone(),
                    f.provider_rule_id.clone(),
                )
            })
        })
        .collect();
    let base = baseline::accept(&entries);

    // … so nothing is "new since baseline".
    assert!(new_findings(&result.comments, &base).is_empty());
    // An empty baseline, by contrast, sees them all as new.
    assert!(!new_findings(&result.comments, &baseline::Baseline::default()).is_empty());
}

#[test]
fn absent_provider_degrades_to_skipped_not_a_crash() {
    // A manifest whose tool does not exist on PATH.
    let manifest = "manifest_version = 1\n\
                    command = [\"cf-absent-tool-xyz123\", \"{files}\"]\n\
                    format = \"json\"\n\
                    scope = \"file\"\n\
                    [capabilities]\n\
                    coordinate_system = \"1-based-utf8\"\n";
    let provider = ManifestProvider::from_toml("absent", manifest).unwrap();

    let overrides = BTreeMap::new();
    let context = ProviderContext::new(Path::new("."), &overrides);
    let run = provider.run(&[PathBuf::from("x.py")], &context);

    // The graceful-degradation contract: tool absent → SKIPPED, findings empty.
    assert_eq!(
        run.state,
        RunState::Skipped,
        "absent tool degrades to SKIPPED"
    );
    assert!(run.findings.is_empty());
}
