---
plan_slug: 2026-06-15-build-commenter-cat-engine
analyzed_at: 2026-06-15T00:00:00Z
stack:
  languages: [rust]
  frameworks: [tree-sitter, rusqlite, rmcp, clap, rayon, ignore, gix, ort]
  test: cargo-test
  linters: [clippy, rustfmt]
  build: [cargo, cargo-dist]
patterns:
  - "Greenfield repo: only Docs/Idea.md, LICENSE, .gitignore, .claude/ exist — no Cargo.toml, no source yet"
  - "Target layout: Cargo workspace with cf-core (substrate types), cf-engine (orchestration library + MCP/render/storage/provider/ops), cf-cli (the `cf` binary)"
  - "MCP verbs are 1:1 with CLI verbs — both share one canonical verb surface on the cf-engine library (Idea §4a)"
  - "Engine is the sole SQLite writer; cache is two layers (content-addressed inputs.db + derived index.db), gitignored at <repo_root>/.comment-finder/"
  - "Rule content is delegated, never reimplemented; providers are adapters (manifest Tier 1 / native Tier 2) behind a RuleProvider trait; built-ins dogfood the manifest format"
  - "Layer discipline: domain code (cf-core) never imports infrastructure (rusqlite/subprocess/git); adapters translate at the seams"
reusable:
  - { module: "Docs/Idea.md", purpose: "Authoritative design spec — §§1-13 + resolved-decisions table; every task cites its section" }
  - { module: "ignore crate", purpose: "File walk + .gitignore/.ignore (ripgrep's engine) — do not hand-roll directory traversal" }
  - { module: "tree-sitter grammars (python/typescript/javascript/bash)", purpose: "Comment extraction + the parse-invariance re-parse — pinned, golden-file-gated" }
  - { module: "rusqlite (bundled SQLite, FTS5, load_extension)", purpose: "Storage + keyword search + sqlite-vec host — bundled, not system SQLite" }
  - { module: "gix (gitoxide)", purpose: "Blame, repo-root resolution, changed-file sets — pure Rust" }
  - { module: "rmcp", purpose: "MCP server SDK for the agent surface (the product)" }
  - { module: "clap", purpose: "CLI verb parsing + JSON output" }
relevant_rules:
  - { path: "~/Workspace/GPT-Prompts/.claude/rules/general.md", focus: "ERR_SWALLOWED, ERR_BARE_RAISE, ERR_NO_CONTEXT (cause-chained context-rich errors); SEC_HARDCODED_SECRET / SEC_SENSITIVE_LOG (gitleaks finds, CF never stores; issue-tracker auth via host creds); SEC_COMMAND_INJECTION (subprocess arg arrays, never shell=true for provider spawns); PERF_UNBOUNDED_COLLECTION (bounded worker/subprocess pools, memory O(workers)); RES_LEAK (RAII/Drop for connections + subprocesses); ARCH_LAYER_VIOLATION (domain cf-core free of infra imports); ORG_MAGIC_NUMBER (τ≈0.8, severity tables, ranking weights as named consts); CONC_RACE (engine sole writer + WAL); API_BREAKING_CHANGE (independently-versioned contracts, MCP stability tiers)" }
  - { path: "~/.claude-work/CLAUDE.md", focus: "Long-term excellence — robust architectural choices over hacky/temporary fixes; present best-approach-only options; zero tolerance for bugs (fix pre-existing issues too); NEVER add Co-Authored-By to commits" }
  - { path: "~/Workspace/GPT-Prompts/.claude/rules/python.md", focus: "Reference-only: applies to the Python FIXTURES that exercise ruff and to test repos, not to the Rust engine itself" }
---

# Codebase Context

## Detected Stack
**Greenfield Rust project.** The repository at plan time contains only `Docs/Idea.md` (the 1056-line design spec), `LICENSE`, `.gitignore` (ignoring `.claude/` + `.DS_Store`), and the `.claude/` working dir — there is **no `Cargo.toml` and no source code**. The Rust toolchain is present (rustc/cargo 1.96.0). The Idea specifies the entire stack (§10): a native-Rust hot path (`tree-sitter` + four grammars · `ignore` · `rayon` · `rusqlite` with bundled SQLite + FTS5 + `sqlite-vec` · `fastembed-rs`/`ort` ONNX · `gix`), the agent surface via `rmcp`, the CLI via `clap`, and an RFC 9535 JSONPath library for manifests. The only non-Rust runtime is the **eslint Node stack**, deliberately isolated as the lone Tier-2 native provider and fetched only when JS/TS is in scope. No Rust-specific rule file exists in the user's rule directories — `general.md` (language-agnostic) governs, with `python.md` applying only to Python test fixtures.

## Patterns to Follow
- **Three-crate workspace:** `cf-core` (pure substrate types: error, version, config, finding, identity) · `cf-engine` (orchestration: walk, extract, map, git, storage, provider, ops, render, mcp, surface, ci, issues, hooks) · `cf-cli` (the `cf` binary). MCP and CLI share the engine library so verbs stay 1:1.
- **Sole-writer storage:** the engine is the only SQLite writer; WAL mode; two-layer cache (content-addressed `inputs.db` survives schema bumps, derived `index.db` rebuilds), gitignored at `<repo_root>/.comment-finder/`.
- **Delegate, don't reimplement:** rule content comes from ruff/eslint/shellcheck/gitleaks via `RuleProvider` adapters; built-ins ship as manifests (dogfooding); `cf` only spawns JSON-mode subprocesses and normalizes.
- **Layer discipline:** `cf-core` (domain) imports no infrastructure; subprocess/SQLite/git errors translate to `CfError` at the `cf-engine` adapter boundary.
- **Determinism + safe-write are the two promises** — every design choice (content-hash keys, canonical ordering, parse-invariance, rebuild-over-migrate) exists to uphold them, and they are property-tested, not asserted.

## Reusable Components
This is greenfield, so there is no prior CF code to reuse; the "reusable" assets are the **authoritative design spec** (`Docs/Idea.md`, cited section-by-section in every task) and the **named third-party crates** the design selects (do not hand-roll their concerns): `ignore` for the walk, the four tree-sitter grammars for extraction *and* the parse-invariance re-parse, `rusqlite`'s bundled SQLite for storage + FTS5 + the sqlite-vec host, `gix` for git, `rmcp` for the MCP surface, and `clap` for the CLI. Within the build, later phases reuse earlier substrate heavily: the parse-invariant applier (7.2) reuses the tree-sitter extraction (2.2); identity Tier-4 (5.3) and hybrid search (4.7) reuse the embeddings (4.5); the round-trip (8.5) reuses `check` (7.1) + the applier (7.2).

## Relevant Rules
The language-agnostic **`general.md`** catalog applies in full to the Rust engine (cited per-area in the frontmatter `relevant_rules` `focus`): cause-chained context-rich errors (`ERR_*`), no swallowed errors, no hardcoded secrets / no secrets in logs (gitleaks *finds* secrets, CF never *stores* them; tracker auth uses host credentials), provider subprocesses spawned with argument arrays never a shell (`SEC_COMMAND_INJECTION`), bounded worker/subprocess pools with memory O(workers) (`PERF_UNBOUNDED_COLLECTION`), RAII cleanup for connections + subprocesses (`RES_LEAK`), domain/infra layer separation (`ARCH_LAYER_VIOLATION`), all thresholds (τ≈0.8, severity tables, ranking weights, timeouts) as named constants (`ORG_MAGIC_NUMBER`), single-writer + WAL for the cache (`CONC_RACE`), and independently-versioned contracts with MCP stability tiers (`API_BREAKING_CHANGE`). The user's global **`~/.claude-work/CLAUDE.md`** mandates long-term-excellence (no hacky/temporary fixes, best-approach-only options, zero tolerance for bugs including pre-existing ones) and **forbids `Co-Authored-By` lines in commits**. Cite rule IDs (path only; full text not pasted).

## Architecture Diagram

```mermaid
graph TB
  subgraph cli["cf-cli (binary)"]
    CLI["clap CLI verbs"]
  end
  subgraph engine["cf-engine (orchestration library)"]
    MCP["MCP surface (rmcp) — the product"]
    SURF["surface: token economy · ranking · round-trip"]
    OPS["ops: check · apply (parse-invariant) · suppress · baseline · fix"]
    PROV["provider: RuleProvider trait · manifest(JSONPath) · eslint native · run-state · pinning"]
    NATIVE["native pass: walk(ignore) · extract+kind(tree-sitter) · coalesce · map→bound_symbol · markers · git(gix) · rot candidates"]
    STORE["storage: inputs.db + index.db (rusqlite/WAL) · FTS5 · sqlite-vec · ONNX embeddings · rebuild"]
    RENDER["render: JSONL · terminal · markdown · SARIF · CSV"]
    IDENT["identity: fingerprint · composite · tiered match"]
    ADJ["adjacent: comment-to-issue · hooks · CI"]
  end
  subgraph core["cf-core (domain types)"]
    TYPES["error · version/comparability-key · config · Finding · Category/severity"]
  end
  subgraph providers["External providers (JSON-mode subprocesses, pinned)"]
    RUFF["ruff · Python"]
    ESLINT["eslint+jsdoc/tsdoc · JS/TS (Node, Tier-2)"]
    SHELL["shellcheck · Shell"]
    LEAKS["gitleaks · secrets"]
  end
  AGENT["External LLM coding agent (the driver)"]

  AGENT <--> MCP
  CLI --> OPS
  MCP --> SURF --> OPS
  OPS --> PROV
  OPS --> NATIVE
  OPS --> IDENT
  OPS --> STORE
  OPS --> RENDER
  PROV -->|normalized Finding| STORE
  NATIVE -->|comment facts| STORE
  PROV --> RUFF
  PROV --> ESLINT
  PROV --> SHELL
  PROV --> LEAKS
  ADJ --> OPS
  engine --> core
  cli --> engine
```
