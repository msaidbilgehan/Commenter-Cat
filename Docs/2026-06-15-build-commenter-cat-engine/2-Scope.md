---
plan_slug: 2026-06-15-build-commenter-cat-engine
section: scope
in_scope:
  - "Rust orchestrator (commenter-cat-core + commenter-cat-engine + commenter-cat-cli workspace) owning the full substrate: walk, tree-sitter extract, kind classification, block coalescing, comment→code mapping, marker extraction, git enrichment, blame-skew rot candidates"
  - "Canonical Finding model: lossless provider_rule_id + canonical_rule_id + category, category-anchored 4-level severity resolution, coordinate conversion (incl. eslint UTF-16), same-category dedup, canonical ordering"
  - "Two-layer SQLite cache: content-addressed inputs.db (survives schema bumps) + derived index.db (rebuilt, never migrated); content-hash keying; FTS5; local ONNX embeddings; sqlite-vec; hybrid RRF retrieval"
  - "Cross-scan identity: cosmetic_fingerprint, composite (bound_symbol, kind, fingerprint), four-tier matching with per-use-case precision"
  - "Manifest-first provider layer: RuleProvider trait + invocation contract, run-state machine, JSONPath ManifestProvider (Tier 1) + generic SARIF ingest, declarative severity/category/capabilities maps, dogfooded built-in manifests (ruff/shellcheck/gitleaks), eslint Tier-2 native + Node tiers, pinning + config-fingerprint + commenter-cat doctor"
  - "Operations: commenter-cat check orchestration + native marker triage, parse-invariant applier, write-protection by kind, filter-up suppression + committed baseline + commenter-cat fix/tighten + opt-in native-directive export"
  - "Interfaces: clap CLI verbs, output renderers (JSONL/terminal/markdown/SARIF/CSV) + CI exit codes, rmcp MCP surface (the product) 1:1 with CLI, token economy, apply/remove round-trip re-check"
  - "Adjacent integrations: comment-to-issue (GitHub/Jira/GitLab pluggable, idempotent), git-hook cache-warmer installer, CI two-artifact cache + degraded-diff verdict"
  - "Distribution + tests: property/golden/contract/integration/reproducibility/MCP/perf tests, OS-matrix CI, cargo-dist with per-platform native-artifact matrix"
out_of_scope:
  - "Any LLM / model inside the engine flow — judgment lives in the consuming agent (Idea §1, §13)"
  - "Reimplementing any per-language linter natively — rule content is delegated to mature tools (Idea §13)"
  - "Semantic-rot detection as an engine capability — engine supplies evidence + candidates + safe apply only (Idea §13)"
  - "Languages beyond Python / TypeScript / JavaScript / Shell — additional tree-sitter grammars are future, low-cost additions (Idea §3)"
  - "Phased rollout, MVP, beta, or 0.x versioning — one complete system shipping at 1.0 (Idea §0)"
  - "API-based / cloud embeddings — local ONNX only; comments never leave the machine (Idea §6, §13)"
  - "Committed/shared index DB — the cache is local + gitignored; shared truth is CI (Idea §6, §13)"
  - "Arbitrary code inside provider manifests — a code-level need crosses the threshold to a Tier-2 native provider (Idea §5, §13)"
constraints:
  - "Rust, edition 2021, MSRV 1.96 (toolchain confirmed present); committed Cargo.lock"
  - "Native hot path is pure Rust (tree-sitter · ignore · rayon · rusqlite with bundled SQLite + FTS5 + sqlite-vec · ort/ONNX); the only non-Rust runtime is the eslint Node stack, isolated as the lone Tier-2 provider, fetched only when JS/TS is in scope"
  - "Repo tool-config is authoritative — Commenter-Cat layers rule-selection + suppression, never overrides a tool's config"
  - "Max-accuracy over zero-setup — external provider dependencies accepted; graceful degradation (on_missing=warn) when a provider is absent"
  - "Providers pinned + auto-managed by default; --system-tools is the explicit escape hatch; baselines updated only under pinned tools"
  - "Sole writer = the engine; SQLite in WAL mode; cache keyed on content/blob hash, never path+mtime"
  - "Independently-versioned contracts (binary · MCP stability tiers · JSONL schema_version · commenter_cat_ruleset_version · manifest_version · config version); cache rebuilds, never migrates; provider bumps are comparability events, not Commenter-Cat breaking changes"
  - "Platforms: Linux · macOS · Windows (x86_64 + aarch64; Windows x86_64); CRLF byte-offset handling explicitly tested"
  - "Apply the general.md rule catalog (errors carry context + cause chains, no swallowed errors, no hardcoded secrets, no command injection via shell, bounded collections, no magic numbers) and the user's long-term-quality philosophy: best-approach only, zero tolerance for bugs"
---

# Scope

## In Scope

The complete `commenter-cat` system as specified in `Docs/Idea.md` §§1–13, organized into ten build-ordered phases. Every capability the design names is built — substrate (extract/map/enrich/normalize/index/search/surface), the canonical finding model, the two-layer cache with hybrid search, cross-scan identity, the manifest-first provider layer with all four built-in providers, the orchestrated `check`/`fix` operations, the parse-invariant safe-write path, the unified filter-up suppression with committed baseline, both interfaces (CLI + the MCP product surface) with the token economy and round-trip, the adjacent integrations (comment-to-issue, hooks, CI), and distribution with the layered proof-grade test suite. See the frontmatter `in_scope` list for the per-area breakdown; each maps to one or more phases.

## Out of Scope

The design's explicitly rejected alternatives (Idea §13) and stated non-goals are out of scope and recorded so they are not re-litigated: no in-engine LLM, no reimplemented linters, no engine-side semantic-rot, no languages beyond the committed four, no MVP/phasing/0.x, no cloud embeddings, no committed index DB, and no arbitrary code in manifests. See the frontmatter `out_of_scope` list with per-item rationale and Idea-section citations.

## Constraints

Grouped:

- **Tech:** Rust edition 2021 / MSRV 1.96, pure-Rust hot path, the eslint Node stack as the single isolated non-Rust runtime, bundled SQLite (WAL, FTS5, sqlite-vec, load_extension), local ONNX embeddings.
- **Architecture / authority:** repo tool-config wins (Commenter-Cat layers, never overrides); engine is the sole writer; content-hash cache keys; independently-versioned contracts with rebuild-over-migrate; max-accuracy with graceful degradation.
- **Reproducibility:** providers pinned + auto-managed by default (`--system-tools` escape hatch), comparability key `(commenter_cat_ruleset_version, provider_version, config_hash)`, byte-identical reports across runs and the OS matrix.
- **Platform:** Linux/macOS/Windows, x86_64 + aarch64, CRLF tested.
- **Quality:** the general.md rule catalog and the user's long-term-excellence philosophy (best-approach-only options, zero tolerance for bugs, no hacky workarounds) govern all implementation.
