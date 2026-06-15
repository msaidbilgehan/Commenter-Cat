//! # cf-engine — Commenter-Cat orchestration library
//!
//! The orchestration layer that drives the substrate (walk · extract · map ·
//! enrich · normalize · index · search) and exposes it through both the CLI
//! ([`cf-cli`](../cf_cli/index.html)) and the agent-facing MCP surface — the
//! product (Idea §4a). MCP verbs are 1:1 with CLI verbs because both call into
//! this one library.
//!
//! This crate is the **sole owner of infrastructure** (SQLite, provider
//! subprocesses, git): it translates infrastructure failures into
//! [`cf_core::CfError`] at each adapter boundary so the domain layer stays pure.
//!
//! Phases 2–10 fill in the modules; Phase 1 establishes only the crate and its
//! dependency on [`cf_core`].

pub mod ci;
pub mod embed;
pub mod extract;
pub mod git;
pub mod hash;
pub mod hooks;
pub mod identity;
pub mod issues;
pub mod map;
pub mod markers;
pub mod mcp;
pub mod ops;
pub mod provider;
pub mod render;
pub mod rot;
pub mod search;
pub mod storage;
pub mod surface;
pub mod walk;

#[cfg(test)]
mod testutil;

/// Returns the engine's contract surface version, sourced from the shared
/// versioned-contract constants in [`cf_core::version`].
///
/// Exists so the binary and tests can confirm the `cf-cli → cf-engine →
/// cf-core` link is wired before any substrate module lands.
#[must_use]
pub fn ruleset_version() -> u32 {
    cf_core::version::CF_RULESET_VERSION
}
