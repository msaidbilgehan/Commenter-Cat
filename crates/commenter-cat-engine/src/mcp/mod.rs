//! The MCP surface (Idea §4a) — THE product.
//!
//! MCP tools are **1:1 with the CLI verbs** (one canonical verb set), so an MCP
//! `check` and a CLI `check` drive the *same* engine entry point; the agent
//! surface then **bounds** its return (Idea §4a — no agent-facing result is a
//! firehose), summarizing + paginating where the CLI renders the full report.
//! [`McpSurface`] is the transport-agnostic dispatcher
//! ([`tools`] is the registry; [`server`] binds it to rmcp over stdio). Read
//! tools run here; index-backed reads and writes resolve a comment id against
//! the persisted index and report clearly until a tree is indexed.

pub mod server;
pub mod tools;

use std::collections::BTreeSet;
use std::path::PathBuf;

use commenter_cat_core::comment::Comment;
use commenter_cat_core::config;
use commenter_cat_core::error::{CommenterCatError, CommenterCatResult};
use commenter_cat_core::finding::Origin;
use commenter_cat_core::kind::CommentKind;
use commenter_cat_core::severity::Severity;
use serde_json::{json, Value};

use crate::ops::index::{self, Session};
use crate::ops::{self};
use crate::provider::{builtins, RuleProvider};
use crate::surface::{ranking, roundtrip, token_economy};

pub use tools::{McpTool, Stability, VerbGroup};

/// The transport-agnostic MCP dispatcher (Idea §4a).
pub struct McpSurface;

impl McpSurface {
    /// The tool descriptors for `tools/list` — name, description, and stability
    /// tier (Idea §11). Order follows [`McpTool::ALL`].
    #[must_use]
    pub fn descriptors() -> Vec<Value> {
        McpTool::ALL
            .iter()
            .map(|tool| {
                json!({
                    "name": tool.name(),
                    "description": tool.description(),
                    "stability": tool.stability().as_str(),
                })
            })
            .collect()
    }

    /// Calls a tool by name with JSON arguments, returning a JSON result.
    ///
    /// # Errors
    /// Returns [`CommenterCatError`] for an unknown tool, a dispatch failure, or a verb
    /// that requires the persisted index before it has been built.
    pub fn call(name: &str, args: &Value) -> CommenterCatResult<Value> {
        let tool = McpTool::from_name(name)
            .ok_or_else(|| CommenterCatError::config(format!("unknown MCP tool {name:?}")))?;
        match tool {
            McpTool::Check => check(args),
            McpTool::Candidates => candidates(args),
            McpTool::Query => query(args),
            McpTool::Context => context(args),
            McpTool::ApplyEdit => apply_edit(args),
            McpTool::Remove => remove(args),
            McpTool::Strip => strip(args),
        }
    }
}

/// The root from `args.path`, or the current directory.
fn root_from(args: &Value) -> CommenterCatResult<PathBuf> {
    match args.get("path").and_then(Value::as_str) {
        Some(path) => Ok(PathBuf::from(path)),
        None => std::env::current_dir().map_err(|e| {
            CommenterCatError::config("determining the working directory").caused_by(e)
        }),
    }
}

/// `check` — run the analysis, persist the unified records, and return a
/// **bounded, ranked summary** (Idea §4a: no agent-facing return is a firehose;
/// the dogfooded `check` dumped ~11M chars on one line and blew the token
/// ceiling). Findings are ranked actionable-first and capped to `limit`
/// (default 50) from `cursor` (default 0), with `total`/`truncated`/`cursor`
/// labels for drilling — the same token economy `candidates` and `query` use.
/// The full records are persisted to the index, so the agent reads bodies and
/// bound code via `query`/`context`, never by parsing a megabyte of `check` JSON.
fn check(args: &Value) -> CommenterCatResult<Value> {
    let root = root_from(args)?;
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .map_or(50, |n| n as usize);
    let offset = args
        .get("cursor")
        .and_then(Value::as_u64)
        .map_or(0, |n| n as usize);
    let config = config::discover(&root)?;
    let providers = builtins::load_all()?;
    let refs: Vec<&dyn RuleProvider> = providers
        .iter()
        .map(|provider| provider as &dyn RuleProvider)
        .collect();
    let result = ops::check::check(&root, &config, &refs, true)?;
    // Persist so the index-backed tools (query/context/apply_edit/remove) resolve.
    index::persist(&root, &result.comments, &index::default_embedder())?;

    let run_states: Vec<Value> = result
        .run_states
        .iter()
        .map(|(provider, state)| json!({ "provider": provider, "state": state.as_str() }))
        .collect();

    // Rank every fused finding actionable-first (severity → blame-age → marker
    // weight), then bound to the budget — the same treatment `candidates` uses.
    let mut ranked: Vec<(ranking::Priority, Value)> = result
        .comments
        .iter()
        .flat_map(|comment| {
            let blame_age = comment.git.as_ref().map_or(0, |g| g.committed_unix.max(0));
            let marker_weight = u32::try_from(comment.markers.len()).unwrap_or(u32::MAX);
            comment.findings.iter().map(move |finding| {
                (
                    ranking::Priority::new(finding.severity, blame_age, marker_weight),
                    json!({
                        "file": finding.file,
                        "line": finding.range.start_line,
                        "severity": finding.severity.as_str(),
                        "category": finding.category.as_str(),
                        "rule": finding.provider_rule_id,
                        "message": finding.message,
                    }),
                )
            })
        })
        .collect();
    ranking::rank_by(&mut ranked, |(priority, _)| *priority);
    let findings: Vec<Value> = ranked.into_iter().map(|(_, value)| value).collect();
    let view = token_economy::bound(
        findings,
        &token_economy::Budget::limit(limit),
        offset,
        |_| 1,
    );

    Ok(json!({
        "summary": {
            "comments": result.comments.len(),
            "findings": view.total,
            "by_severity": severity_histogram(&result),
            "unattached": result.unattached.len(),
            "suppressed": result.suppressed.len(),
        },
        "run_states": run_states,
        "total": view.total,
        "truncated": view.truncated,
        "cursor": view.cursor.map(|c| c.offset),
        "findings": view.items,
    }))
}

/// A `{critical, error, warning, info}` count over every fused finding, for the
/// `check` summary — a fixed-shape severity breakdown the agent reads at a glance.
fn severity_histogram(result: &ops::check::CheckResult) -> Value {
    let (mut critical, mut error, mut warning, mut info) = (0usize, 0usize, 0usize, 0usize);
    for finding in result.comments.iter().flat_map(|comment| &comment.findings) {
        match finding.severity {
            Severity::Critical => critical += 1,
            Severity::Error => error += 1,
            Severity::Warning => warning += 1,
            Severity::Info => info += 1,
        }
    }
    json!({ "critical": critical, "error": error, "warning": warning, "info": info })
}

/// `candidates` — the native worklist, ranked actionable-first and token-bounded.
fn candidates(args: &Value) -> CommenterCatResult<Value> {
    let root = root_from(args)?;
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .map_or(20, |n| n as usize);
    let config = config::discover(&root)?;
    let no_providers: [&dyn RuleProvider; 0] = [];
    let result = ops::check::check(&root, &config, &no_providers, true)?;

    let mut items: Vec<(ranking::Priority, Value)> = result
        .comments
        .iter()
        .flat_map(|comment| {
            comment
                .findings
                .iter()
                .filter(|f| f.origin == Origin::Native)
                .map(move |finding| {
                    let blame_age = comment.git.as_ref().map_or(0, |g| g.committed_unix.max(0));
                    let priority = ranking::Priority::new(
                        finding.severity,
                        blame_age,
                        u32::try_from(comment.markers.len()).unwrap_or(u32::MAX),
                    );
                    (
                        priority,
                        json!({
                            "file": finding.file,
                            "line": finding.range.start_line,
                            "rule": finding.canonical_rule_id,
                            "severity": finding.severity.as_str(),
                            "message": finding.message,
                        }),
                    )
                })
        })
        .collect();

    ranking::rank_by(&mut items, |(priority, _)| *priority);
    let ranked: Vec<Value> = items.into_iter().map(|(_, value)| value).collect();
    let view = token_economy::bound(ranked, &token_economy::Budget::limit(limit), 0, |_| 1);

    Ok(json!({
        "total": view.total,
        "truncated": view.truncated,
        "cursor": view.cursor.map(|c| c.offset),
        "candidates": view.items,
    }))
}

/// `query` — FIND: search the persisted index, ranked + token-bounded.
fn query(args: &Value) -> CommenterCatResult<Value> {
    let session = Session::open(&root_from(args)?)?;
    let text = args
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .map_or(20, |n| n as usize);

    let hits = session.query(text, limit)?;
    let results: Vec<Value> = hits
        .iter()
        .map(|(id, comment)| {
            json!({
                "id": id,
                "file": comment.path,
                "line": comment.range.start_line,
                "kind": comment.kind.as_str(),
                "findings": comment.findings.len(),
                "summary": comment.raw_text.lines().next().unwrap_or_default().trim(),
            })
        })
        .collect();
    Ok(json!({ "total": results.len(), "results": results }))
}

/// `context` — UNDERSTAND: one comment, with its bound code only on request
/// (never bundled into `query`, Idea §4a token economy).
fn context(args: &Value) -> CommenterCatResult<Value> {
    let session = Session::open(&root_from(args)?)?;
    let comment = resolve(&session, args)?;
    let mut result = json!({
        "file": comment.path,
        "line": comment.range.start_line,
        "kind": comment.kind.as_str(),
        "bound_symbol": comment.bound_symbol.as_ref().map(|s| s.as_str()),
        "text": comment.raw_text,
        "findings": comment.findings,
    });
    if args
        .get("with_code")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        if let Some(range) = comment.bound_node_range {
            let source = session.read_source(&comment)?;
            let (start, end) = (range.start_byte as usize, range.end_byte as usize);
            if let Some(code) = source.get(start..end) {
                result["code"] = json!(code);
            }
        }
    }
    Ok(result)
}

/// `apply_edit` — UPDATE: a parse-invariant edit, written to disk, re-checked.
fn apply_edit(args: &Value) -> CommenterCatResult<Value> {
    let session = Session::open(&root_from(args)?)?;
    let comment = resolve(&session, args)?;
    let new_text = args
        .get("new_text")
        .and_then(Value::as_str)
        .ok_or_else(|| CommenterCatError::config("apply_edit requires `new_text`"))?;
    let allow_significant = args
        .get("allow_significant")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let source = session.read_source(&comment)?;
    let config = config::discover(session.repo_root())?;
    let no_providers: [&dyn RuleProvider; 0] = [];

    let trip = roundtrip::apply_edit_and_recheck(
        &source,
        &comment,
        new_text,
        allow_significant,
        session.repo_root(),
        &config,
        &no_providers,
    )?;
    session.write_source(&comment, &trip.new_source)?;
    Ok(json!({ "applied": true, "significant": trip.significant, "findings": trip.findings }))
}

/// `remove` — UPDATE: remove a comment, written to disk, re-checked.
fn remove(args: &Value) -> CommenterCatResult<Value> {
    let session = Session::open(&root_from(args)?)?;
    let comment = resolve(&session, args)?;
    let allow_significant = args
        .get("allow_significant")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let source = session.read_source(&comment)?;
    let config = config::discover(session.repo_root())?;
    let no_providers: [&dyn RuleProvider; 0] = [];

    let trip = roundtrip::remove_and_recheck(
        &source,
        &comment,
        allow_significant,
        session.repo_root(),
        &config,
        &no_providers,
    )?;
    session.write_source(&comment, &trip.new_source)?;
    Ok(json!({ "removed": true, "findings": trip.findings }))
}

/// `strip` — UPDATE: sweep the tree and delete every comment the policy permits
/// (Idea §4a, §5).
///
/// The one tool that rewrites source in bulk, so it is **dry-run by default**:
/// without `apply=true` it returns the complete plan and touches nothing. The
/// per-file list is ranked by removal count and bounded by `limit` like every
/// other agent-facing return, while the summary counts stay over the full set.
/// `reindex_required` tells the agent the persisted index is now stale and a
/// `check` call will re-derive it.
fn strip(args: &Value) -> CommenterCatResult<Value> {
    let root = root_from(args)?;
    let config = config::discover(&root)?;
    let apply = flag(args, "apply", false);
    let policy = ops::strip::StripPolicy {
        allow_significant: flag(args, "allow_significant", false),
        strip_license: flag(args, "strip_license", false),
        keep_kinds: keep_kinds(args)?,
        tidy: flag(args, "tidy", true),
    };
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .map_or(50, |n| n as usize);

    let report = ops::strip::strip(&root, &config, &policy, !apply)?;

    // Biggest cleanups first, so page one is where the sweep actually landed.
    let mut files: Vec<&ops::strip::StrippedFile> = report.files.iter().collect();
    files.sort_by(|a, b| b.removed.cmp(&a.removed).then_with(|| a.path.cmp(&b.path)));
    let files: Vec<Value> = files
        .into_iter()
        .map(|file| json!({ "path": file.path, "removed": file.removed, "lines": file.lines }))
        .collect();
    let view = token_economy::bound(files, &token_economy::Budget::limit(limit), 0, |_| 1);

    Ok(json!({
        "dry_run": report.dry_run,
        "summary": {
            "files_scanned": report.files_scanned,
            "comments_scanned": report.comments_scanned,
            "removed": report.removed,
            "kept": {
                "significant": report.kept_significant,
                "license": report.kept_license,
                "by_kind": report.kept_by_kind,
            },
            "files_touched": report.files.len(),
        },
        "reindex_required": report.reindex_required(),
        "total": view.total,
        "truncated": view.truncated,
        "cursor": view.cursor.map(|c| c.offset),
        "files": view.items,
        "skipped": report.skipped,
    }))
}

/// A boolean argument with its default.
fn flag(args: &Value, name: &str, default: bool) -> bool {
    args.get(name).and_then(Value::as_bool).unwrap_or(default)
}

/// The `keep` argument: comment-kind tokens the strip pass must preserve.
fn keep_kinds(args: &Value) -> CommenterCatResult<BTreeSet<CommentKind>> {
    let Some(values) = args.get("keep").and_then(Value::as_array) else {
        return Ok(BTreeSet::new());
    };
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .and_then(CommentKind::from_token)
                .ok_or_else(|| {
                    let known: Vec<&str> = CommentKind::ALL.iter().map(|k| k.as_str()).collect();
                    CommenterCatError::config(format!(
                        "unknown comment kind {value} in `keep` (expected one of: {})",
                        known.join(", ")
                    ))
                })
        })
        .collect()
}

/// Resolves the `comment_id` argument to its persisted comment record.
fn resolve(session: &Session, args: &Value) -> CommenterCatResult<Comment> {
    let id = args
        .get("comment_id")
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<i64>().ok())
        .ok_or_else(|| {
            CommenterCatError::config("a numeric `comment_id` (from `query`) is required")
        })?;
    session
        .comment(id)?
        .ok_or_else(|| CommenterCatError::config(format!("no comment {id} in the index")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TestRepo;

    #[test]
    fn test_descriptors_carry_name_and_stability() {
        let descriptors = McpSurface::descriptors();
        assert_eq!(descriptors.len(), 7);
        assert_eq!(descriptors[0]["name"], "query");
        assert_eq!(descriptors[0]["stability"], "stable");
        let apply = descriptors
            .iter()
            .find(|d| d["name"] == "apply_edit")
            .unwrap();
        assert_eq!(apply["stability"], "experimental");
    }

    #[test]
    fn test_check_tool_returns_a_bounded_ranked_summary() {
        let repo = TestRepo::new();
        repo.write("m.py", "# TODO clean up\nx = 1\n");
        let args = json!({ "path": repo.path().to_string_lossy() });

        let result = McpSurface::call("check", &args).unwrap();
        // Idea §4a: check returns a bounded summary, never the full record dump.
        assert!(
            result["findings"].is_array(),
            "findings is a ranked, bounded list"
        );
        assert!(result["run_states"].is_array());
        assert_eq!(result["summary"]["comments"], 1);
        // The TODO marker is one finding, surfaced and counted.
        assert!(result["total"].as_u64().unwrap() >= 1);
        assert_eq!(result["summary"]["findings"], result["total"]);
        assert!(
            result.get("comments").is_none(),
            "the full-record firehose key is gone"
        );
    }

    #[test]
    fn test_check_findings_are_token_bounded() {
        let repo = TestRepo::new();
        repo.write("m.py", "# TODO one\n# FIXME two\n# HACK three\nx = 1\n");
        let args = json!({ "path": repo.path().to_string_lossy(), "limit": 1 });

        let result = McpSurface::call("check", &args).unwrap();
        // Several markers found, but the budget caps the returned slice at one.
        assert!(
            result["total"].as_u64().unwrap() >= 2,
            "more findings exist than were returned"
        );
        assert_eq!(
            result["findings"].as_array().unwrap().len(),
            1,
            "the budget caps the slice"
        );
        assert_eq!(result["truncated"], true, "truncation is labeled");
        assert!(result["cursor"].is_number(), "a drill cursor is provided");
    }

    #[test]
    fn test_candidates_tool_is_bounded() {
        let repo = TestRepo::new();
        repo.write("m.py", "# TODO one\n# FIXME two\nx = 1\n");
        let args = json!({ "path": repo.path().to_string_lossy(), "limit": 1 });

        let result = McpSurface::call("candidates", &args).unwrap();
        // Two markers found, but the budget caps the returned slice at 1.
        assert!(result["total"].as_u64().unwrap() >= 2);
        assert_eq!(result["candidates"].as_array().unwrap().len(), 1);
        assert_eq!(result["truncated"], true);
    }

    #[test]
    fn test_strip_tool_plans_by_default_and_applies_on_request() {
        let repo = TestRepo::new();
        repo.write("m.py", "# a note\nx = 1\n");
        let path = json!(repo.path().to_string_lossy());

        // Default: a plan. The summary is populated but the file is untouched —
        // the destructive path is opt-in, never the default for an agent.
        let plan = McpSurface::call("strip", &json!({ "path": path })).unwrap();
        assert_eq!(plan["dry_run"], true);
        assert_eq!(plan["summary"]["removed"], 1);
        assert_eq!(plan["reindex_required"], false);
        assert_eq!(plan["files"][0]["path"], "m.py");
        assert_eq!(
            std::fs::read_to_string(repo.path().join("m.py")).unwrap(),
            "# a note\nx = 1\n"
        );

        // apply=true rewrites, and says the index now needs re-deriving.
        let applied = McpSurface::call("strip", &json!({ "path": path, "apply": true })).unwrap();
        assert_eq!(applied["dry_run"], false);
        assert_eq!(applied["reindex_required"], true);
        assert_eq!(
            std::fs::read_to_string(repo.path().join("m.py")).unwrap(),
            "x = 1\n"
        );
    }

    #[test]
    fn test_strip_keeps_protected_kinds_and_bounds_its_file_list() {
        let repo = TestRepo::new();
        repo.write("a.py", "#!/usr/bin/env python3\n# one\n# two\nx = 1\n");
        repo.write("b.py", "# three\ny = 2\n");
        let args = json!({ "path": repo.path().to_string_lossy(), "limit": 1 });

        let plan = McpSurface::call("strip", &args).unwrap();

        assert_eq!(plan["summary"]["kept"]["significant"], 1, "the shebang");
        assert_eq!(
            plan["total"].as_u64().unwrap(),
            2,
            "both files have removals"
        );
        assert_eq!(
            plan["files"].as_array().unwrap().len(),
            1,
            "bounded to limit"
        );
        assert_eq!(plan["truncated"], true);
    }

    #[test]
    fn test_strip_rejects_an_unknown_keep_kind() {
        let repo = TestRepo::new();
        repo.write("m.py", "# a note\nx = 1\n");
        let args = json!({ "path": repo.path().to_string_lossy(), "keep": ["nonsense"] });
        let error = McpSurface::call("strip", &args).expect_err("unknown kind is refused");
        assert!(error.to_string().contains("nonsense"), "{error}");
    }

    #[test]
    fn test_unknown_tool_errors() {
        assert!(McpSurface::call("nope", &json!({})).is_err());
    }

    #[test]
    fn test_find_understand_update_loop_end_to_end() {
        let repo = TestRepo::new();
        repo.write("m.py", "# stale note about the cache\nx = 1\n");
        let path = json!(repo.path().to_string_lossy());

        // check builds + persists the index.
        McpSurface::call("check", &json!({ "path": path })).unwrap();

        // query finds the comment and returns its id.
        let found = McpSurface::call("query", &json!({ "path": path, "query": "cache" })).unwrap();
        let id = found["results"][0]["id"]
            .as_i64()
            .expect("a hit with an id");

        // context fetches it with bound code on request.
        let ctx = McpSurface::call(
            "context",
            &json!({ "path": path, "comment_id": id.to_string(), "with_code": true }),
        )
        .unwrap();
        assert!(ctx["text"].as_str().unwrap().contains("cache"));

        // apply_edit writes a parse-invariant edit and returns re-checked findings;
        // the new text introduces a TODO, which the inline re-check surfaces.
        let edited = McpSurface::call(
            "apply_edit",
            &json!({ "path": path, "comment_id": id.to_string(), "new_text": "# TODO revisit the cache" }),
        )
        .unwrap();
        assert_eq!(edited["applied"], true);
        let findings = edited["findings"].as_array().unwrap();
        assert!(findings
            .iter()
            .any(|f| f["message"].as_str().unwrap_or_default().contains("TODO")));
    }
}
