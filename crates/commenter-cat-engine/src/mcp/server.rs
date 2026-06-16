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
use rmcp::{tool, tool_handler, tool_router, ErrorData, ServerHandler};
use serde_json::Value;

use commenter_cat_core::error::{cause_chain, CommenterCatError, CommenterCatResult};

use super::McpSurface;

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
        Parameters(args): Parameters<Value>,
    ) -> Result<CallToolResult, ErrorData> {
        dispatch("query", args)
    }

    #[tool(
        name = "context",
        description = "Fetch one comment; include its bound code on request."
    )]
    async fn context(
        &self,
        Parameters(args): Parameters<Value>,
    ) -> Result<CallToolResult, ErrorData> {
        dispatch("context", args)
    }

    #[tool(
        name = "check",
        description = "Run the comment analysis over the tree and return findings."
    )]
    async fn check(
        &self,
        Parameters(args): Parameters<Value>,
    ) -> Result<CallToolResult, ErrorData> {
        dispatch("check", args)
    }

    #[tool(
        name = "candidates",
        description = "The native worklist: rot candidates and ranked markers."
    )]
    async fn candidates(
        &self,
        Parameters(args): Parameters<Value>,
    ) -> Result<CallToolResult, ErrorData> {
        dispatch("candidates", args)
    }

    #[tool(
        name = "apply_edit",
        description = "Apply a parse-invariant comment edit, then re-check inline."
    )]
    async fn apply_edit(
        &self,
        Parameters(args): Parameters<Value>,
    ) -> Result<CallToolResult, ErrorData> {
        dispatch("apply_edit", args)
    }

    #[tool(
        name = "remove",
        description = "Remove a comment, then re-check inline."
    )]
    async fn remove(
        &self,
        Parameters(args): Parameters<Value>,
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

/// Delegates a tool call to the shared surface, returning the JSON result as a
/// text content block (raw `CallToolResult`, so rmcp generates no output schema
/// for the arbitrary JSON shape). A `CommenterCatError` maps to an MCP error carrying the
/// cause chain.
fn dispatch(name: &str, args: Value) -> Result<CallToolResult, ErrorData> {
    let to_mcp_err = |e: CommenterCatError| ErrorData::internal_error(cause_chain(&e), None);
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
}
