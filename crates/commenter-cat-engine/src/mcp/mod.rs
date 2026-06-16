//! The MCP surface (Idea §4a) — THE product.
//!
//! MCP tools are **1:1 with the CLI verbs** (one canonical verb set), so an MCP
//! `check` and a CLI `check` call the *same* engine entry point and return the
//! *same* record shape. [`McpSurface`] is the transport-agnostic dispatcher
//! ([`tools`] is the registry; [`server`] binds it to rmcp over stdio). Read
//! tools run here; index-backed reads and writes resolve a comment id against
//! the persisted index and report clearly until a tree is indexed.

pub mod server;
pub mod tools;

use std::path::PathBuf;

use commenter_cat_core::comment::Comment;
use commenter_cat_core::config;
use commenter_cat_core::error::{CommenterCatError, CommenterCatResult};
use commenter_cat_core::finding::Origin;
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

/// `check` — the same engine entry point the CLI verb calls (Idea §4a).
fn check(args: &Value) -> CommenterCatResult<Value> {
    let root = root_from(args)?;
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
    Ok(json!({ "comments": result.comments, "run_states": run_states }))
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
        assert_eq!(descriptors.len(), 6);
        assert_eq!(descriptors[0]["name"], "query");
        assert_eq!(descriptors[0]["stability"], "stable");
        let apply = descriptors
            .iter()
            .find(|d| d["name"] == "apply_edit")
            .unwrap();
        assert_eq!(apply["stability"], "experimental");
    }

    #[test]
    fn test_check_tool_returns_same_shape_as_engine() {
        let repo = TestRepo::new();
        repo.write("m.py", "# TODO clean up\nx = 1\n");
        let args = json!({ "path": repo.path().to_string_lossy() });

        let result = McpSurface::call("check", &args).unwrap();
        // The MCP result wraps the SAME comment records the engine produced.
        assert!(result["comments"].is_array());
        let comments = result["comments"].as_array().unwrap();
        assert_eq!(comments.len(), 1);
        assert!(result["run_states"].is_array());
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
