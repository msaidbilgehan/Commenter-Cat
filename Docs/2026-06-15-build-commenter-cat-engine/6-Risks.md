---
plan_slug: 2026-06-15-build-commenter-cat-engine
section: risks
risks:
  - id: R1
    description: "sqlite-vec + ONNX (ort) per-platform native artifacts must be version-matched to the bundled SQLite/ort across five targets; a mismatch or missing artifact breaks load_extension or inference at runtime."
    likelihood: high
    impact: high
    mitigation: "Pin and version-match per (os, arch) in cargo-dist; CI builds + attaches all five and FAILS the release on a missing artifact (Idea §10). Test load_extension on each OS in the matrix (10.4); enable load_extension in rusqlite's bundled SQLite (4.6)."
  - id: R2
    description: "The eslint Node stack is the lone non-Rust runtime — version drift across eslint/parser/plugins/Node and lockfile pinning is the hardest reproducibility surface."
    likelihood: high
    impact: medium
    mitigation: "Isolate as the single Tier-2 native provider, fetched only when JS/TS is in scope; pin a full lockfile tree; two Node tiers (SEMI_HERMETIC default / --hermetic); record reproducibility_level in run metadata; config_hash over `eslint --print-config` (6.6, 6.7)."
  - id: R3
    description: "Parse-invariance must hold for ALL comment edits across four grammars; an edge case (CRLF, nested block comments, docstring-as-expression, unicode) could let a code-node change slip through — a safe-write breach."
    likelihood: medium
    impact: high
    mitigation: "The guarantee is PROPERTY-tested with proptest over arbitrary edits, not example-tested (10.1); abort on any code-node delta; CRLF explicitly tested (2.2, 10.4). This is a load-bearing test by design (Idea §11)."
  - id: R4
    description: "RFC 9535 JSONPath dialect differences between candidate Rust libraries could make manifests non-portable/non-deterministic."
    likelihood: medium
    impact: medium
    mitigation: "Pin one concrete RFC 9535 library at build (Idea §14 implementation-tuning); contract-test manifest extraction against recorded fixtures (10.2); forbid arbitrary code in manifests so behavior stays declarative (6.3)."
  - id: R5
    description: "Cross-scan identity Tier-4 fuzzy matching (similarity ≥ τ≈0.8) could mis-match and, if ever wired to suppression, hide a real finding."
    likelihood: low
    impact: high
    mitigation: "Architecturally forbid Tier-4 from suppression (suppression/baseline use Tier-2 cosmetic only); property-test identity stability (cosmetic edit preserves Tier-2, marker escalation breaks it) (5.3, 10.1). τ as a named, tuned const."
  - id: R6
    description: "Coordinate reconciliation is error-prone — eslint reports UTF-16 columns while tree-sitter/ruff use byte/char, with 1- vs 0-based offsets per tool; an off-by-one corrupts ranges, suppression, and apply targeting."
    likelihood: medium
    impact: medium
    mitigation: "Centralize conversion in one module (3.3) with a declared-convention enum per provider; unit + property test UTF-16↔byte with multi-byte chars (3.3, 10.1); each adapter declares its coordinate_system in [capabilities] (6.4)."
  - id: R7
    description: "Provider tools (ruff/eslint/shellcheck/gitleaks) change rule IDs, JSON shape, and severity scales each release — ongoing maintenance drift."
    likelihood: high
    impact: low
    mitigation: "Manifest-first turns most upkeep into a declarative TOML edit + a contract-test run (Idea §11); provider_rule_id is lossless; contract tests catch shape breaks (10.2); provider bumps are comparability events, not CF breaking changes."
open_questions:
  - id: Q1
    question: "Which concrete RFC 9535 JSONPath crate and which SARIF→Finding mapping table are adopted?"
    decision_needed_by_phase: 6
    blocking: false
  - id: Q2
    question: "What are the default ranking weights (severity/blame-age/marker), the default `limit`, and the `context` token budget for the agent surface?"
    decision_needed_by_phase: 8
    blocking: false
  - id: Q3
    question: "What are the exact rayon worker count, subprocess-pool size, and per-provider timeout values?"
    decision_needed_by_phase: 6
    blocking: false
  - id: Q4
    question: "Which embedding model (and its ONNX export + version) is bundled for local embeddings?"
    decision_needed_by_phase: 4
    blocking: false
assumptions:
  - id: A1
    assumption: "Greenfield: the plan creates the entire Cargo workspace from scratch; no existing crate layout to honor."
    confirmed_by: "codebase analysis at 2026-06-15T00:00:00Z — only Docs/, LICENSE, .gitignore, .claude/ present; no Cargo.toml"
  - id: A2
    assumption: "Rust edition 2021, MSRV 1.96."
    confirmed_by: "rustc/cargo 1.96.0 confirmed present in the environment; Idea §10 specifies Rust"
  - id: A3
    assumption: "The crate split is cf-core (types) + cf-engine (orchestration incl. MCP) + cf-cli (binary), with MCP and CLI sharing the engine library so verbs stay 1:1."
    confirmed_by: "Idea §4a (MCP verbs 1:1 with CLI) + §10 (cf binary, rmcp surface) — a conservative naming/layout default recorded per the planner minor-unclarity rule"
  - id: A4
    assumption: "The product/binary keeps the name Commenter-Cat with the `cf` binary, `cf:` directive, and `.commenter-cat/` cache dir (the design's chosen, reversible naming)."
    confirmed_by: "Idea §12 Name decision — kept for coherence across ~20 'CF' usages"
  - id: A5
    assumption: "No Rust-specific user rule file exists; general.md governs the Rust engine, python.md applies only to Python test fixtures."
    confirmed_by: "ls of ~/Workspace/GPT-Prompts/.claude/rules/ at analysis time — only general.md, python.md, react.md"
  - id: A6
    assumption: "Implementation-tuning values (worker counts, timeouts, ranking weights, JSONPath lib, embedding model) are decided in code during the relevant phase, not in this plan."
    confirmed_by: "Idea §14 explicitly defers these to implementation-tuning"
rollback:
  strategy: "Greenfield build, so rollback is per-task git revert — each task is a focused, independently-revertable change to new files. The runtime cache is rebuildable + gitignored (never a rollback concern). The only non-rebuildable artifact is the committed baseline (commenter-cat.baseline.toml), which migrates in place via `cf baseline migrate` and is read current + N-1. STATUS.md tracks per-task waypoints; a failed task returns to queued (attempt++)."
  trigger_states: [failed, blocked]
---

# Risks, Open Questions, and Assumptions

## Risks
- **R1 — Native-artifact matrix (high/high):** the per-platform sqlite-vec + ONNX artifacts are "the real packaging work" (Idea §10). Version-match per target; CI fails the release on a missing artifact and tests `load_extension` on each OS. Monitoring signal: release CI artifact-presence check + per-OS extension-load test.
- **R2 — eslint Node stack (high/medium):** the single non-Rust runtime and the hardest reproducibility surface. Isolated as the lone Tier-2 provider, full lockfile pin, two Node tiers, `reproducibility_level` surfaced. Signal: `cf doctor` config-differs + `reproducibility_level` in run metadata.
- **R3 — Parse-invariance edge cases (medium/high):** a safe-write breach if any code-node change slips through. Property-tested over arbitrary edits (the load-bearing proptest), abort on any delta, CRLF tested. Signal: proptest failures + the abort counter.
- **R4 — JSONPath portability (medium/medium):** dialect differences could break manifest determinism. Pin one RFC 9535 lib; contract-test against fixtures; no code in manifests. Signal: manifest contract-test diffs.
- **R5 — Tier-4 fuzzy mis-match (low/high):** would hide a real finding if wired to suppression. Architecturally forbidden from suppression; identity stability property-tested. Signal: identity proptest + a suppression-tier assertion.
- **R6 — Coordinate reconciliation (medium/medium):** UTF-16 vs byte, 1- vs 0-based. Centralized conversion module, property-tested with multi-byte chars, per-adapter declared convention. Signal: coordinate unit/property tests.
- **R7 — Provider tool drift (high/low):** rule IDs/shape/severity change each release. Manifest-first makes upkeep declarative; lossless rule IDs; contract tests catch breaks; bumps are comparability events. Signal: provider contract-test breaks + `cf doctor`.

## Open Questions
None are blocking — all are Idea §14 implementation-tuning decisions made in-code during the relevant phase:
- **Q1** (JSONPath lib + SARIF mapping) — by Phase 6.
- **Q2** (ranking weights, default `limit`, `context` token budget) — by Phase 8, tuned against real agent sessions.
- **Q3** (rayon workers, subprocess-pool size, per-provider timeouts) — by Phase 6.
- **Q4** (bundled embedding model + ONNX export/version) — by Phase 4.

## Assumptions
All confirmable from the codebase analysis or the Idea spec (see frontmatter `confirmed_by`): greenfield workspace creation (A1), Rust 2021 / MSRV 1.96 (A2), the three-crate split with MCP/CLI sharing the engine (A3, a conservative default per the minor-unclarity rule), the kept Commenter-Cat/`cf` naming (A4), `general.md` as the governing rule set (A5), and implementation-tuning values decided in-code (A6).

## Rollback Strategy
Greenfield build → rollback is a per-task `git revert` of focused changes to new files; tasks are independently revertable. The runtime two-layer cache is rebuildable and gitignored, never a rollback concern (Idea §6). The only non-rebuildable artifact is the **committed baseline** (`commenter-cat.baseline.toml`), which migrates in place (`cf baseline migrate`, read current + N-1, Idea §11). `STATUS.md` tracks per-task waypoints; a failed task returns to `queued` with `attempt` incremented.
