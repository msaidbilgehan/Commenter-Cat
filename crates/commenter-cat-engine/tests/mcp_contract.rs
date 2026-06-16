//! MCP agent-contract tests (Idea §4a, §11).
//!
//! The promises an agent relies on: every return is **token-bounded** (a budget
//! caps the slice and labels truncation with a drill cursor), the `apply_edit`
//! **round-trip** returns the re-checked findings inline (one call deep), and
//! **write-protection by kind** refuses a behavior-bearing edit without an
//! explicit acknowledgement.

// Test code: unwrap/expect on known-good fixtures is idiomatic.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::process::Command;

use commenter_cat_core::config::ResolvedConfig;
use commenter_cat_core::kind::CommentKind;
use commenter_cat_core::lang::Language;
use commenter_cat_engine::extract::extract_source;
use commenter_cat_engine::mcp::McpSurface;
use commenter_cat_engine::ops::apply::apply_edit;
use commenter_cat_engine::provider::RuleProvider;
use commenter_cat_engine::surface::roundtrip::apply_edit_and_recheck;
use serde_json::json;

fn repo_with(file: &str, contents: &str) -> tempfile::TempDir {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(dir.path().join(file), contents).unwrap();
    Command::new("git")
        .arg("init")
        .arg("-q")
        .current_dir(dir.path())
        .status()
        .unwrap();
    dir
}

#[test]
fn candidates_return_is_token_bounded() {
    // Two markers, but a budget of one → a labeled, cursored, partial view.
    let dir = repo_with("m.py", "# TODO one\n# FIXME two\nx = 1\n");
    let args = json!({ "path": dir.path().to_string_lossy(), "limit": 1 });

    let result = McpSurface::call("candidates", &args).unwrap();
    assert!(
        result["total"].as_u64().unwrap() >= 2,
        "more candidates exist than were returned"
    );
    assert_eq!(
        result["candidates"].as_array().unwrap().len(),
        1,
        "the budget caps the slice"
    );
    assert_eq!(result["truncated"], true, "truncation is labeled");
    assert!(result["cursor"].is_number(), "a drill cursor is provided");
}

#[test]
fn apply_edit_round_trip_returns_rechecked_findings() {
    let source = "# clean note\nx = 1\n";
    let comment = extract_source(source, Language::Python, Path::new("a.py"), "a.py")
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    let config = ResolvedConfig::default();
    let providers: [&dyn RuleProvider; 0] = [];

    // The edit introduces a TODO; the round-trip re-checks the touched comment
    // and returns the newly-introduced finding inline (no separate `check`).
    let trip = apply_edit_and_recheck(
        source,
        &comment,
        "# TODO revisit",
        false,
        Path::new("/tmp/commenter-cat-mcp-contract"),
        &config,
        &providers,
    )
    .unwrap();

    assert!(trip.new_source.contains("# TODO revisit"));
    assert!(
        trip.findings.iter().any(|f| f.message.contains("TODO")),
        "the round-trip surfaces the finding the edit just introduced"
    );
}

#[test]
fn write_protection_refuses_behavior_bearing_edit_without_ack() {
    // A `# type: ignore` directive is behavior-bearing (Idea §4a).
    let source = "x = bad()  # type: ignore\n";
    let directive = extract_source(source, Language::Python, Path::new("a.py"), "a.py")
        .unwrap()
        .into_iter()
        .find(|c| c.kind == CommentKind::Directive)
        .expect("the type-ignore directive is classified as such");

    // Editing it without `allow_significant` is refused …
    assert!(apply_edit(source, &directive, "# type: check", false).is_err());
    // … but permitted (and flagged significant) with the acknowledgement.
    let acknowledged = apply_edit(source, &directive, "# type: check", true).unwrap();
    assert!(
        acknowledged.significant,
        "the edit is flagged behavior-bearing"
    );
}
