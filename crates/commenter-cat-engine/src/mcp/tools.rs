//! The MCP tool registry (Idea §4a) — THE product surface.
//!
//! The six primitives, **1:1 with the CLI verbs** (one canonical verb set, two
//! interfaces). Each tool declares its find/understand/rule-check/update group
//! and a per-tool **stability tier** (Idea §11): a new capability lands
//! `Experimental`; agents pin a MAJOR and rely on `Stable` tools.

use std::fmt;

/// Surface stability tier (Idea §11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stability {
    /// Covered by SemVer within the pinned MAJOR.
    Stable,
    /// May change shape before stabilizing.
    Experimental,
}

impl Stability {
    /// The lowercase token (for the tool descriptor / capability listing).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Stability::Stable => "stable",
            Stability::Experimental => "experimental",
        }
    }
}

/// The verb group a tool belongs to (Idea §4a).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerbGroup {
    /// `query`, `candidates` — locate comments.
    Find,
    /// `context` — fetch one comment + bound code.
    Understand,
    /// `check` — run the analysis.
    RuleCheck,
    /// `apply_edit`, `remove` — write.
    Update,
}

/// The six MCP primitives (Idea §4a), each named exactly like its CLI verb.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpTool {
    /// FIND: search comments (FTS + vector), ranked + bounded.
    Query,
    /// UNDERSTAND: fetch one comment, with bound code on request.
    Context,
    /// RULE-CHECK: run the analysis over the tree.
    Check,
    /// FIND: the native worklist (rot + markers).
    Candidates,
    /// UPDATE: parse-invariant comment edit + inline re-check.
    ApplyEdit,
    /// UPDATE: remove a comment + inline re-check.
    Remove,
}

impl McpTool {
    /// Every tool, in surface order.
    pub const ALL: [McpTool; 6] = [
        McpTool::Query,
        McpTool::Context,
        McpTool::Check,
        McpTool::Candidates,
        McpTool::ApplyEdit,
        McpTool::Remove,
    ];

    /// The tool name — **identical** to the CLI verb (snake_case for MCP).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            McpTool::Query => "query",
            McpTool::Context => "context",
            McpTool::Check => "check",
            McpTool::Candidates => "candidates",
            McpTool::ApplyEdit => "apply_edit",
            McpTool::Remove => "remove",
        }
    }

    /// The verb group (Idea §4a).
    #[must_use]
    pub const fn group(self) -> VerbGroup {
        match self {
            McpTool::Query | McpTool::Candidates => VerbGroup::Find,
            McpTool::Context => VerbGroup::Understand,
            McpTool::Check => VerbGroup::RuleCheck,
            McpTool::ApplyEdit | McpTool::Remove => VerbGroup::Update,
        }
    }

    /// The stability tier (Idea §11). The read primitives are stable; the write
    /// round-trip is still experimental until its shape settles.
    #[must_use]
    pub const fn stability(self) -> Stability {
        match self {
            McpTool::Query | McpTool::Context | McpTool::Check | McpTool::Candidates => {
                Stability::Stable
            }
            McpTool::ApplyEdit | McpTool::Remove => Stability::Experimental,
        }
    }

    /// A one-line description for the tool descriptor.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            McpTool::Query => "Search comments by text (FTS + vector), ranked and token-bounded.",
            McpTool::Context => "Fetch one comment; include its bound code on request.",
            McpTool::Check => "Run the comment analysis over the tree and return findings.",
            McpTool::Candidates => "The native worklist: rot candidates and ranked markers.",
            McpTool::ApplyEdit => "Apply a parse-invariant comment edit, then re-check inline.",
            McpTool::Remove => "Remove a comment, then re-check inline.",
        }
    }

    /// Resolves a tool by name, or `None` if unknown.
    #[must_use]
    pub fn from_name(name: &str) -> Option<McpTool> {
        McpTool::ALL.into_iter().find(|tool| tool.name() == name)
    }
}

impl fmt::Display for McpTool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_names_match_cli_verbs_and_round_trip() {
        // The six primitives, named exactly as the CLI verbs (Idea §4a).
        let names: Vec<&str> = McpTool::ALL.iter().map(|t| t.name()).collect();
        assert_eq!(
            names,
            vec![
                "query",
                "context",
                "check",
                "candidates",
                "apply_edit",
                "remove"
            ]
        );
        for tool in McpTool::ALL {
            assert_eq!(McpTool::from_name(tool.name()), Some(tool));
        }
        assert_eq!(McpTool::from_name("nope"), None);
    }

    #[test]
    fn test_write_primitives_are_experimental() {
        assert_eq!(McpTool::Check.stability(), Stability::Stable);
        assert_eq!(McpTool::ApplyEdit.stability(), Stability::Experimental);
        assert_eq!(McpTool::Remove.stability(), Stability::Experimental);
    }

    #[test]
    fn test_groups() {
        assert_eq!(McpTool::Query.group(), VerbGroup::Find);
        assert_eq!(McpTool::Context.group(), VerbGroup::Understand);
        assert_eq!(McpTool::Check.group(), VerbGroup::RuleCheck);
        assert_eq!(McpTool::Remove.group(), VerbGroup::Update);
    }
}
