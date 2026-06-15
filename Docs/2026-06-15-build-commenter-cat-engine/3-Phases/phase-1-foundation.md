---
plan_slug: 2026-06-15-build-commenter-cat-engine
phase: 1
name: foundation
goal: "Establish the Cargo workspace, error hierarchy, versioned-contract constants, and the layered TOML config model that every other crate depends on."
depends_on_phases: []
parallel_safe_with_phases: []
tasks:
  - id: "1.1"
    name: scaffold-cargo-workspace
    action: "Create a Cargo workspace at the repo root with member crates cf-core (substrate types), cf-engine (orchestration library), and cf-cli (the `cf` binary); set edition 2021, rust-version 1.96, MSRV pin, shared [workspace.dependencies], a committed Cargo.lock, and a workspace-level rustfmt.toml + clippy lint table (deny warnings)."
    files: [Cargo.toml, Cargo.lock, rustfmt.toml, crates/cf-core/Cargo.toml, crates/cf-core/src/lib.rs, crates/cf-engine/Cargo.toml, crates/cf-engine/src/lib.rs, crates/cf-cli/Cargo.toml, crates/cf-cli/src/main.rs]
    depends_on: []
    parallel_safe: false
    validation: "cargo build --workspace succeeds and `cargo metadata` lists cf-core, cf-engine, cf-cli"
    notes: "Greenfield: nothing exists but Docs/. This task creates the entire crate skeleton. Idea §10 names the dependency stack; do NOT add provider/MCP deps yet — only workspace scaffolding."
  - id: "1.2"
    name: define-error-hierarchy
    action: "Create the domain error hierarchy in cf-core: a root CfError enum with thiserror, subgrouped variants (ConfigError, WalkError, ExtractError, MapError, StorageError, ProviderError, ApplyError, IdentityError) each carrying context fields; every variant uses #[from]/#[source] to preserve cause chains (general.md ERR_BARE_RAISE)."
    files: [crates/cf-core/src/error.rs, crates/cf-core/src/lib.rs]
    depends_on: ["1.1"]
    parallel_safe: false
    validation: "cargo test -p cf-core error:: passes; clippy reports no `unwrap()`/`expect()` in non-test error paths"
    notes: "general.md ERR_SWALLOWED/ERR_NO_CONTEXT: errors must carry operation + sanitized inputs. Boundary errors (provider subprocess, sqlite) translate to CfError at the adapter (ARCH_LAYER_VIOLATION)."
  - id: "1.3"
    name: define-versioned-contracts
    action: "Create cf-core::version module defining the independently-versioned contract constants from Idea §11: SCHEMA_VERSION_JSONL, INPUTS_DB_SCHEMA_VERSION, INDEX_DB_SCHEMA_VERSION, CF_RULESET_VERSION, MANIFEST_VERSION (=1), CONFIG_VERSION (=1), plus a ComparabilityKey type bundling (cf_ruleset_version, provider_version, config_hash)."
    files: [crates/cf-core/src/version.rs, crates/cf-core/src/lib.rs]
    depends_on: ["1.1"]
    parallel_safe: true
    validation: "cargo test -p cf-core version::test_comparability_key_equality passes"
    notes: "Idea §11 table: conflating these contracts is the trap; each is a distinct const. ComparabilityKey is reused by storage (§6) and providers (§5)."
  - id: "1.4"
    name: implement-config-model
    action: "Create the TOML config model in cf-core (serde structs for [scan], [providers], [markers], [severity], [search], [output] per the Idea §12 sketch) with walk-up + hierarchical cascade discovery, CF_* env overrides, XDG global layer, a config `version` field validated against CONFIG_VERSION (unknown future version = hard error), and a resolved-config struct distinct from the on-disk struct."
    files: [crates/cf-core/src/config/mod.rs, crates/cf-core/src/config/discovery.rs, crates/cf-core/src/config/model.rs]
    depends_on: ["1.2", "1.3"]
    parallel_safe: false
    validation: "cargo test -p cf-core config:: passes, covering walk-up cascade, CF_* override precedence, and unknown-version hard error"
    notes: "Idea §12 Configuration sketch is the authoritative shape. Use pydantic-equivalent boundary validation (serde + explicit validators). Do NOT use mutable defaults; no magic numbers (general.md ORG_MAGIC_NUMBER)."
---

# Phase 1: Foundation

## Goal
Stand up the Cargo workspace and the cross-cutting primitives every later crate imports: the error hierarchy, the independently-versioned contract constants, and the layered TOML configuration model. This is greenfield — the repo currently contains only `Docs/`, `LICENSE`, and `.gitignore`, so this phase creates the entire crate skeleton. Nothing here delegates to a provider or touches SQLite; it is pure substrate scaffolding so that Phases 2 (native substrate) and 3 (finding model) can begin in parallel.

## Tasks

### 1.1 — scaffold-cargo-workspace
- **Action:** Create a Cargo workspace at the repo root with member crates `cf-core`, `cf-engine`, `cf-cli`; edition 2021, rust-version 1.96, shared `[workspace.dependencies]`, committed `Cargo.lock`, `rustfmt.toml`, clippy deny-warnings table.
- **Files:** `Cargo.toml`, `Cargo.lock`, `rustfmt.toml`, `crates/cf-core/{Cargo.toml,src/lib.rs}`, `crates/cf-engine/{Cargo.toml,src/lib.rs}`, `crates/cf-cli/{Cargo.toml,src/main.rs}`
- **Depends on:** none
- **Validation:** `cargo build --workspace` succeeds; `cargo metadata` lists all three crates.
- The three-crate split (`cf-core` types · `cf-engine` orchestration · `cf-cli` binary) keeps the MCP server (§4a, later a feature of cf-engine) and CLI on the same library, honoring "MCP verbs 1:1 with CLI verbs."

### 1.2 — define-error-hierarchy
- **Action:** Create the `CfError` domain hierarchy in cf-core with `thiserror`, context-carrying variants, and preserved cause chains.
- **Files:** `crates/cf-core/src/error.rs`, `crates/cf-core/src/lib.rs`
- **Depends on:** 1.1
- **Validation:** `cargo test -p cf-core error::` passes; clippy finds no `unwrap`/`expect` in error paths.
- Boundary errors (subprocess, SQLite, git) translate to `CfError` at adapters — domain code never imports `rusqlite::Error` directly (ARCH_LAYER_VIOLATION).

### 1.3 — define-versioned-contracts
- **Action:** Define the independently-versioned contract constants and the `ComparabilityKey` type.
- **Files:** `crates/cf-core/src/version.rs`, `crates/cf-core/src/lib.rs`
- **Depends on:** 1.1
- **Validation:** `cargo test -p cf-core version::test_comparability_key_equality` passes.
- Distinct from 1.2 (different file), so 1.3 is parallel-safe with 1.2.

### 1.4 — implement-config-model
- **Action:** Create the layered TOML config model with cascade discovery, env overrides, and version validation.
- **Files:** `crates/cf-core/src/config/{mod.rs,discovery.rs,model.rs}`
- **Depends on:** 1.2, 1.3
- **Validation:** `cargo test -p cf-core config::` passes (cascade, env precedence, unknown-version error).
- The Idea §12 sketch is the authoritative config shape.

## Phase Validation
`cargo build --workspace && cargo test -p cf-core && cargo clippy --workspace -- -D warnings` all pass; the workspace compiles with the error, version, and config modules exported from `cf-core`'s public API.
