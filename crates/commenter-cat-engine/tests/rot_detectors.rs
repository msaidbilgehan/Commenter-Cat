//! End-to-end proof that the five native silent-rot detectors surface through
//! `commenter-cat check` on real tree-sitter + git fixtures (plan task 5.4).
//!
//! Real files, real grammars, real git — one true positive per structural
//! detector, plus an all-accurate fixture that must stay silent and a two-run
//! determinism check. The semantic detector is default-off, so it is excluded
//! from these assertions.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::process::Command;

use commenter_cat_core::config::ResolvedConfig;
use commenter_cat_core::finding::Finding;
use commenter_cat_engine::ops::check::{check, CheckResult};
use commenter_cat_engine::provider::RuleProvider;

/// The native-only provider set (no external tools).
const NO_PROVIDERS: [&dyn RuleProvider; 0] = [];

/// The canonical rule ids the rot detectors emit.
const ROT_RULES: [&str; 5] = [
    "rot_ref",
    "rot_path",
    "rot_signature",
    "rot_drift",
    "rot_semantic",
];

/// Runs a git command isolated from the machine's global/system config.
fn git(dir: &Path, args: &[&str], date: Option<&str>) {
    let mut command = Command::new("git");
    command
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "Tester")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "Tester")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .env("GIT_CONFIG_GLOBAL", dir.join("__no_global__"))
        .env("GIT_CONFIG_SYSTEM", dir.join("__no_system__"));
    if let Some(date) = date {
        command
            .env("GIT_AUTHOR_DATE", date)
            .env("GIT_COMMITTER_DATE", date);
    }
    assert!(command.status().expect("git is available").success());
}

/// Builds a throwaway git repo seeded with `files`, initialized but not committed.
fn repo_with(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::TempDir::new().unwrap();
    for (path, contents) in files {
        let full = dir.path().join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(full, contents).unwrap();
    }
    git(dir.path(), &["init", "-q"], None);
    dir
}

/// Writes one file relative to `dir`.
fn write(dir: &Path, path: &str, contents: &str) {
    let full = dir.join(path);
    if let Some(parent) = full.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(full, contents).unwrap();
}

/// Stages everything and commits at `date`.
fn commit_all(dir: &Path, date: &str) {
    git(dir, &["add", "-A"], None);
    git(
        dir,
        &["-c", "commit.gpgsign=false", "commit", "-q", "-m", "snap"],
        Some(date),
    );
}

/// The rot findings (by canonical rule id) across a check result.
fn rot_findings(result: &CheckResult) -> Vec<&Finding> {
    result
        .comments
        .iter()
        .flat_map(|comment| &comment.findings)
        .filter(|finding| ROT_RULES.contains(&finding.canonical_rule_id.as_str()))
        .collect()
}

/// Whether a finding with `rule` is present.
fn has_rule(result: &CheckResult, rule: &str) -> bool {
    rot_findings(result)
        .iter()
        .any(|finding| finding.canonical_rule_id == rule)
}

#[test]
fn structural_detectors_each_surface_a_true_positive() {
    // refs.py — a comment naming a call that does not exist (rot_ref).
    // paths.py — a comment naming a missing file (rot_path).
    // sig.py — a docstring documenting a renamed parameter (rot_signature).
    let dir = repo_with(&[
        (
            "refs.py",
            "# mirrors gone_helper() which was deleted upstream\ndef present():\n    return present\n",
        ),
        (
            "paths.py",
            "# the lawyer-authored copy lives at account_deletion_requested.html\nVALUE = 1\n",
        ),
        (
            "sig.py",
            "def connect(timeout_s):\n    \"\"\"Open a connection.\n\n    Args:\n        timeout: seconds to wait\n    \"\"\"\n    return do_connect(timeout_s)\n",
        ),
    ]);

    let result = check(dir.path(), &ResolvedConfig::default(), &NO_PROVIDERS, false).unwrap();

    assert!(has_rule(&result, "rot_ref"), "dangling reference surfaces");
    assert!(has_rule(&result, "rot_path"), "missing path surfaces");
    assert!(
        has_rule(&result, "rot_signature"),
        "renamed parameter surfaces"
    );

    // Every rot finding is native + agent-judged, anchored to a comment.
    for finding in rot_findings(&result) {
        assert_eq!(
            finding.origin,
            commenter_cat_core::finding::Origin::Native,
            "rot findings are native"
        );
        assert_eq!(
            finding.fix,
            commenter_cat_core::finding::Fix::AgentOnly,
            "the agent judges; the engine proposes"
        );
    }
}

#[test]
fn git_drift_surfaces_when_code_changes_after_the_comment() {
    let dir = repo_with(&[(
        "drift.py",
        "# explains compute\ndef compute():\n    return 1\n",
    )]);
    commit_all(dir.path(), "2020-01-01 00:00:00 +0000");
    // The comment is untouched; only compute's body changes, two years later.
    write(
        dir.path(),
        "drift.py",
        "# explains compute\ndef compute():\n    return 2\n",
    );
    commit_all(dir.path(), "2022-01-01 00:00:00 +0000");

    let result = check(dir.path(), &ResolvedConfig::default(), &NO_PROVIDERS, false).unwrap();
    assert!(
        has_rule(&result, "rot_drift"),
        "code newer than its comment by > 30 days surfaces as drift"
    );
}

#[test]
fn accurate_fixture_surfaces_no_rot_findings() {
    // A resolved reference, an existing path, and a faithful docstring → silence.
    let dir = repo_with(&[(
        "good.py",
        "# see `helper` and good.py for the algorithm\ndef helper():\n    \"\"\"Do the work.\"\"\"\n    return 1\n",
    )]);
    commit_all(dir.path(), "2021-06-01 00:00:00 +0000");

    let result = check(dir.path(), &ResolvedConfig::default(), &NO_PROVIDERS, false).unwrap();
    let findings = rot_findings(&result);
    assert!(
        findings.is_empty(),
        "an accurate fixture manufactures no rot findings: {:?}",
        findings.iter().map(|f| &f.message).collect::<Vec<_>>()
    );
}

#[test]
fn rot_findings_are_deterministic_across_runs() {
    let files = &[
        (
            "refs.py",
            "# mirrors gone_helper() which was deleted\ndef present():\n    return present\n",
        ),
        (
            "paths.py",
            "# template at account_deletion_requested.html\nVALUE = 1\n",
        ),
    ];
    let first = repo_with(files);
    let second = repo_with(files);

    let run = |dir: &Path| -> Vec<(String, String)> {
        let result = check(dir, &ResolvedConfig::default(), &NO_PROVIDERS, false).unwrap();
        rot_findings(&result)
            .iter()
            .map(|finding| (finding.canonical_rule_id.clone(), finding.message.clone()))
            .collect()
    };

    let a = run(first.path());
    let b = run(second.path());
    assert!(!a.is_empty(), "the fixture does surface findings");
    assert_eq!(a, b, "identical fixtures yield identical rot findings");
}
