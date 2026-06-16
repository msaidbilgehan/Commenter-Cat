---
plan_slug: 2026-06-15-build-commenter-cat-engine
phase: 8
name: interfaces
goal: "Expose the engine: the clap CLI verbs, the output renderers (JSONL/terminal/markdown/SARIF/CSV) with CI exit codes, the rmcp MCP surface (the product) 1:1 with CLI verbs, the token economy, and the apply/remove round-trip re-check."
depends_on_phases: [7]
parallel_safe_with_phases: []
tasks:
  - id: "8.1"
    name: implement-cli-verbs
    action: "Create the clap CLI in commenter-cat-cli wiring the verbs to commenter-cat-engine ops: query, context, check, candidates, apply_edit, remove (the six primitives) plus baseline accept/prune, suppressions export, doctor, install-hooks, issues sync; --stats, --strict, --system-tools, --hermetic, --show-suppressed flags."
    files: [crates/commenter-cat-cli/src/main.rs, crates/commenter-cat-cli/src/cli/mod.rs, crates/commenter-cat-cli/src/cli/verbs.rs]
    depends_on: []
    parallel_safe: true
    validation: "cargo test -p commenter-cat-cli cli:: passes: each verb parses with its flags and dispatches to the right engine entry point; `commenter-cat --help` lists all verbs"
    notes: "Idea §4a: MCP verbs are 1:1 with CLI verbs, so the CLI is the canonical verb surface both interfaces share. clap (§10). Six primitives organized under find/understand/rule-check/update (§4a)."
  - id: "8.2"
    name: implement-output-renderers
    action: "Create the output renderers in commenter-cat-engine: JSONL (the canonical schema_version-tagged stream), terminal (grouped by tag/file/author/age), markdown reports, SARIF (GitHub code-scanning), CSV; plus CI exit codes that fail the build on findings at/above fail_on and on DO_NOT_MERGE / stale todo-or-die."
    files: [crates/commenter-cat-engine/src/render/mod.rs, crates/commenter-cat-engine/src/render/jsonl.rs, crates/commenter-cat-engine/src/render/terminal.rs, crates/commenter-cat-engine/src/render/sarif.rs, crates/commenter-cat-engine/src/render/markdown_csv.rs]
    depends_on: ["8.1"]
    parallel_safe: false
    validation: "cargo test -p commenter-cat-engine render:: passes: JSONL round-trips with schema_version, SARIF validates against the schema, and exit code is non-zero when a finding ≥ fail_on is present"
    notes: "Idea §8 Output formats: structured-first, JSONL canonical, everything else a renderer. SARIF here is the EMIT side (§8); the ingest side is Phase 6 (6.3). Different file from 8.1 → sequenced after CLI defines the verb outputs."
  - id: "8.3"
    name: implement-mcp-surface
    action: "Create the MCP server in commenter-cat-engine using rmcp, exposing the six primitives as MCP tools 1:1 with the CLI verbs (query, context, check, candidates, apply_edit, remove) under the three-verb organization (find/understand/rule-check/update), each carrying a per-verb stability tier (stable/experimental)."
    files: [crates/commenter-cat-engine/src/mcp/mod.rs, crates/commenter-cat-engine/src/mcp/tools.rs, crates/commenter-cat-engine/src/mcp/server.rs]
    depends_on: ["8.1"]
    parallel_safe: true
    validation: "cargo test -p commenter-cat-engine mcp:: passes: each MCP tool maps to its CLI verb's engine entry point and reports its stability tier; an MCP `query` returns the same result shape as the CLI"
    notes: "Idea §4a: THE PRODUCT. rmcp Rust SDK (§10). Surface stability tiers (§11): new capability lands experimental first; agents pin a MAJOR. Different subtree from 8.2 → parallel-safe."
  - id: "8.4"
    name: implement-token-economy
    action: "Implement the token economy across query/check returns: summaries by default (ranked head = total count + top-N) with a cursor, bound code opt-in via context (never bundled into query), a budget-aware limit/max_tokens cap returning the highest-priority slice that fits plus a truncated flag + cursor, and actionable-first ranking (severity → blame-age → marker weight, or FTS/vector score for search)."
    files: [crates/commenter-cat-engine/src/surface/token_economy.rs, crates/commenter-cat-engine/src/surface/ranking.rs, crates/commenter-cat-engine/src/surface/mod.rs]
    depends_on: ["8.3", "8.2"]
    parallel_safe: false
    validation: "cargo test -p commenter-cat-engine surface::token_economy passes: a result over budget returns a labeled-truncated slice + cursor, ranking is actionable-first, and context is required to fetch bound code"
    notes: "Idea §4a Token economy — the first-class constraint: no return is a firehose; a bounded view is always labeled. Shared by both CLI (8.2) and MCP (8.3) → depends on both. Ranking weights are impl-tuned (§14) but defaults are named consts."
  - id: "8.5"
    name: implement-roundtrip-recheck
    action: "Implement the apply_edit/remove round-trip: after a comment-only write or deletion, re-run check for just the touched comment/symbol and RETURN the re-checked findings inline, so the find→update→re-check cycle is one tool-call deep without a separate check call."
    files: [crates/commenter-cat-engine/src/surface/roundtrip.rs, crates/commenter-cat-engine/src/surface/mod.rs]
    depends_on: ["8.4"]
    parallel_safe: false
    validation: "cargo test -p commenter-cat-engine surface::roundtrip passes: apply_edit returns the re-checked findings for the touched comment, including a finding the edit just introduced"
    notes: "Idea §4a 'The round trip closes the loop.' Reuses the applier (7.2) + check (7.1) scoped to one comment/symbol. This makes the live-session loop one call deep. Shares surface/ with 8.4 → sequential."
---

# Phase 8: Interfaces

## Goal
Expose the engine through its two interfaces — the CLI and, crucially, the MCP surface that *is the product* (Idea §4a). Both share one canonical verb set (MCP tools 1:1 with CLI verbs). This phase adds the output renderers with CI exit codes, the token economy that keeps every agent-facing return ranked-bounded-drillable (the first-class constraint), and the `apply_edit`/`remove` round-trip that closes the find→update→re-check loop in a single tool call. Depends on Phase 7 (all operations the verbs invoke).

## Tasks

### 8.1 — implement-cli-verbs
- **Action:** clap CLI wiring the six primitives + baseline/suppressions/doctor/install-hooks/issues verbs + flags.
- **Files:** `crates/commenter-cat-cli/src/main.rs`, `crates/commenter-cat-cli/src/cli/{mod.rs,verbs.rs}`
- **Depends on:** none (within phase)
- **Validation:** each verb parses + dispatches; `commenter-cat --help` lists all verbs.

### 8.2 — implement-output-renderers
- **Action:** JSONL (canonical, schema_version-tagged) + terminal + markdown + SARIF (emit) + CSV + CI exit codes.
- **Files:** `crates/commenter-cat-engine/src/render/{mod.rs,jsonl.rs,terminal.rs,sarif.rs,markdown_csv.rs}`
- **Depends on:** 8.1
- **Validation:** JSONL round-trips; SARIF validates; exit code non-zero at ≥ fail_on.

### 8.3 — implement-mcp-surface
- **Action:** rmcp MCP server exposing the six primitives 1:1 with CLI verbs + per-verb stability tiers.
- **Files:** `crates/commenter-cat-engine/src/mcp/{mod.rs,tools.rs,server.rs}`
- **Depends on:** 8.1 (parallel-safe with 8.2 — different subtree)
- **Validation:** each MCP tool maps to its verb's engine entry point + reports tier.

### 8.4 — implement-token-economy
- **Action:** Summaries-by-default + cursor, opt-in bound code via context, budget-aware truncation+flag, actionable-first ranking.
- **Files:** `crates/commenter-cat-engine/src/surface/{token_economy.rs,ranking.rs,mod.rs}`
- **Depends on:** 8.3, 8.2
- **Validation:** over-budget → labeled-truncated slice + cursor; ranking actionable-first; context required for bound code.

### 8.5 — implement-roundtrip-recheck
- **Action:** `apply_edit`/`remove` return re-checked findings for the touched comment/symbol inline.
- **Files:** `crates/commenter-cat-engine/src/surface/{roundtrip.rs,mod.rs}`
- **Depends on:** 8.4
- **Validation:** apply_edit returns re-checked findings, including a newly introduced one.

## Phase Validation
`cargo test -p commenter-cat-cli cli:: && cargo test -p commenter-cat-engine render:: mcp:: surface::` all pass; a fixture session can be driven through both the CLI and the MCP surface — running `query`/`check` with token-budgeted ranked output, drilling via `context`, and completing an `apply_edit` that returns its re-checked findings in one call — proving the live-session loop (§4a) end to end.
