//! The MCP stdio server, bound to the rmcp SDK (Idea §4a, §10).
//!
//! The transport is rmcp; the product is unchanged. Each tool is a thin `#[tool]`
//! method that delegates to [`super::McpSurface`] — the same dispatch the CLI
//! verbs share — so the wire framing evolves with the SDK without touching the
//! tool surface, the registry, or the engine. Served over stdio.

// The rmcp `#[tool_router]`/`#[tool_handler]` macros generate transport-binding
// code we don't own; scope the lint relaxations they need to this module.
#![allow(clippy::needless_pass_by_value, clippy::unused_async)]

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, ServerCapabilities, ServerInfo};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_handler, tool_router, ErrorData, ServerHandler};
use serde::{Deserialize, Serialize};

use commenter_cat_core::error::{cause_chain, CommenterCatError, CommenterCatResult};

use super::McpSurface;

// Typed parameter schemas for the six tools. Deriving `JsonSchema` makes the
// rmcp `#[tool]` macro emit a proper `{"type":"object", …}` inputSchema with
// per-field documentation. Without a typed parameter, an untyped `Value`
// argument derives the permissive `AnyValue` schema (no `"type"`), which strict
// MCP clients — Claude Code and the Anthropic API — reject; the rejection drops
// the *entire* tool list, so the server connects but exposes nothing. Each
// struct is deserialized at the transport edge, then re-serialized to a `Value`
// for the shared [`McpSurface`] dispatch, so the CLI and MCP keep one engine
// entry point. `schemars(crate = "rmcp::schemars")` targets rmcp's re-export, so
// there is no direct `schemars` dependency to drift from the one the macro uses.

/// Arguments for `query`.
#[derive(Serialize, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct QueryArgs {
    /// Repository root to operate on. Defaults to the server's working directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    /// Free-text search over comment bodies (full-text + vector). Omit to match broadly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    query: Option<String>,
    /// Maximum number of results to return. Defaults to 20.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    limit: Option<u64>,
}

/// Arguments for `context`.
#[derive(Serialize, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct ContextArgs {
    /// Repository root to operate on. Defaults to the server's working directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    /// Numeric comment id returned by `query`, passed as a string (e.g. "42").
    comment_id: String,
    /// When true, include the bound code span alongside the comment. Defaults to false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    with_code: Option<bool>,
}

/// Arguments for `check`.
#[derive(Serialize, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct CheckArgs {
    /// Repository root to operate on. Defaults to the server's working directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    /// Maximum findings in the ranked, bounded slice. Defaults to 50. The summary
    /// counts are always over the full set; this caps only the returned `findings`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    limit: Option<u64>,
    /// Drill cursor (offset) returned by a prior page, to fetch the next slice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cursor: Option<u64>,
}

/// Arguments for `candidates`.
#[derive(Serialize, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct CandidatesArgs {
    /// Repository root to operate on. Defaults to the server's working directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    /// Maximum number of candidates to return. Defaults to 20.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    limit: Option<u64>,
}

/// Arguments for `apply_edit`.
#[derive(Serialize, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct ApplyEditArgs {
    /// Repository root to operate on. Defaults to the server's working directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    /// Numeric comment id returned by `query`, passed as a string (e.g. "42").
    comment_id: String,
    /// Replacement comment text, including the comment delimiters (e.g. "# …").
    new_text: String,
    /// Permit edits to behavior-bearing comments (directive, shebang, encoding-decl). Defaults to false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    allow_significant: Option<bool>,
}

/// Arguments for `remove`.
#[derive(Serialize, Deserialize, JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
struct RemoveArgs {
    /// Repository root to operate on. Defaults to the server's working directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    /// Numeric comment id returned by `query`, passed as a string (e.g. "42").
    comment_id: String,
    /// Permit removing behavior-bearing comments (directive, shebang, encoding-decl). Defaults to false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    allow_significant: Option<bool>,
}

/// The Commenter-Cat MCP server — the six primitives, 1:1 with the CLI verbs.
/// The `#[tool_router]`/`#[tool_handler]` macros generate the routing and a
/// tools-enabled `get_info` (server name/version inferred from `Cargo.toml`).
#[derive(Clone, Default)]
pub struct CommenterCatServer;

#[tool_router]
impl CommenterCatServer {
    /// Builds the server.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    #[tool(
        name = "query",
        description = "Search comments by text (FTS + vector), ranked and token-bounded."
    )]
    async fn query(
        &self,
        Parameters(args): Parameters<QueryArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        dispatch("query", args)
    }

    #[tool(
        name = "context",
        description = "Fetch one comment; include its bound code on request."
    )]
    async fn context(
        &self,
        Parameters(args): Parameters<ContextArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        dispatch("context", args)
    }

    #[tool(
        name = "check",
        description = "Run the comment analysis over the tree and return findings."
    )]
    async fn check(
        &self,
        Parameters(args): Parameters<CheckArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        dispatch("check", args)
    }

    #[tool(
        name = "candidates",
        description = "The native worklist: rot candidates and ranked markers."
    )]
    async fn candidates(
        &self,
        Parameters(args): Parameters<CandidatesArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        dispatch("candidates", args)
    }

    #[tool(
        name = "apply_edit",
        description = "Apply a parse-invariant comment edit, then re-check inline."
    )]
    async fn apply_edit(
        &self,
        Parameters(args): Parameters<ApplyEditArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        dispatch("apply_edit", args)
    }

    #[tool(
        name = "remove",
        description = "Remove a comment, then re-check inline."
    )]
    async fn remove(
        &self,
        Parameters(args): Parameters<RemoveArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        dispatch("remove", args)
    }
}

#[tool_handler]
impl ServerHandler for CommenterCatServer {
    /// Identifies the server as `commenter-cat` (not the SDK) and advertises the tools
    /// capability. Providing `get_info` suppresses the macro's default.
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::new(ServerCapabilities::builder().enable_tools().build());
        info.server_info.name = "commenter-cat".to_owned();
        info.server_info.version = env!("CARGO_PKG_VERSION").to_owned();
        info
    }
}

/// Delegates a tool call to the shared surface. The typed `args` are serialized
/// back to JSON for [`McpSurface::call`] — the same dispatch the CLI verbs use —
/// and the JSON result is returned as a text content block (a raw
/// `CallToolResult`, so rmcp generates no output schema for the arbitrary JSON
/// shape). A `CommenterCatError` maps to an MCP error carrying the cause chain.
fn dispatch<T: Serialize>(name: &str, args: T) -> Result<CallToolResult, ErrorData> {
    let to_mcp_err = |e: CommenterCatError| ErrorData::internal_error(cause_chain(&e), None);
    let args = serde_json::to_value(args).map_err(|e| {
        to_mcp_err(CommenterCatError::render("serializing MCP tool arguments").caused_by(e))
    })?;
    let value = McpSurface::call(name, &args).map_err(to_mcp_err)?;
    let text = serde_json::to_string(&value).map_err(|e| {
        to_mcp_err(CommenterCatError::render("serializing MCP tool result").caused_by(e))
    })?;
    Ok(CallToolResult::success(vec![Content::text(text)]))
}

/// Serves the MCP protocol over stdio until the client disconnects (Idea §4a).
///
/// Builds a private async runtime so callers stay synchronous — the engine is
/// sync; only this transport edge is async.
///
/// # Errors
/// Returns [`CommenterCatError::Render`] if the runtime or transport fails.
pub fn serve_stdio() -> CommenterCatResult<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| CommenterCatError::render("building the MCP async runtime").caused_by(e))?;
    runtime.block_on(serve())
}

/// The async stdio serve loop.
async fn serve() -> CommenterCatResult<()> {
    use rmcp::transport::stdio;
    use rmcp::ServiceExt;

    let service = CommenterCatServer::new()
        .serve(stdio())
        .await
        .map_err(|e| CommenterCatError::render(format!("starting the MCP server: {e}")))?;
    service
        .waiting()
        .await
        .map_err(|e| CommenterCatError::render(format!("MCP server run error: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TestRepo;
    use serde_json::json;

    #[test]
    fn test_get_info_enables_the_tools_capability() {
        let info = CommenterCatServer::new().get_info();
        assert!(
            info.capabilities.tools.is_some(),
            "the server advertises tools"
        );
    }

    #[test]
    fn test_dispatch_delegates_to_the_surface() {
        let repo = TestRepo::new();
        repo.write("m.py", "# TODO clean up\nx = 1\n");
        let args = json!({ "path": repo.path().to_string_lossy() });
        // The `check` tool runs through the shared surface and returns a
        // non-error content result.
        let result = dispatch("check", args).expect("check dispatches");
        assert_ne!(result.is_error, Some(true), "check is a success result");
    }

    #[test]
    fn test_dispatch_maps_errors_to_mcp_errors() {
        assert!(dispatch("nope", json!({})).is_err());
    }

    #[test]
    fn test_tool_input_schemas_are_objects() {
        // Regression guard: every tool's inputSchema must be a JSON Schema with
        // `"type": "object"`. An untyped `Value` parameter derives a schema with
        // no `type`, which strict MCP clients reject — silently dropping the
        // whole tool list. The typed parameter structs keep each schema valid.
        let tools = CommenterCatServer::tool_router().list_all();
        assert_eq!(tools.len(), 6, "all six primitives are registered");
        for tool in tools {
            let schema = &tool.input_schema;
            assert_eq!(
                schema.get("type").and_then(|t| t.as_str()),
                Some("object"),
                "tool {:?} inputSchema must be an object schema, got {schema:?}",
                tool.name
            );
        }
    }
}
