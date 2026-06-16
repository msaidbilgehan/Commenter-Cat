---
plan_slug: 2026-06-15-build-commenter-cat-engine
phase: 7
name: operations
goal: "Orchestrate the unified `commenter-cat check`, the parse-invariant safe-apply with write-protection by kind, the filter-up suppression model with committed baseline, provider-autofix delegation, and native-directive export."
depends_on_phases: [5, 6]
parallel_safe_with_phases: []
tasks:
  - id: "7.1"
    name: implement-commenter-cat-check-orchestration
    action: "Create the `commenter-cat check` orchestration in commenter-cat-engine: run providers only on cache-miss files (Phase 4 cache), normalize every tool's output + add native findings (blame-skew candidates from 2.7, cross-language marker triage = marker × severity × blame-age), fuse into findings[] attached to comments by location + bound_symbol, and persist into the index."
    files: [crates/commenter-cat-engine/src/ops/check.rs, crates/commenter-cat-engine/src/ops/normalize.rs, crates/commenter-cat-engine/src/ops/triage.rs]
    depends_on: []
    parallel_safe: true
    validation: "cargo test -p commenter-cat-engine ops::check passes on a multi-language fixture: provider findings + native candidates + marker triage fuse into one unified result attached to the right comments"
    notes: "Idea §5 `commenter-cat check` + §9 Comment triage. The normalizer fuses provider Findings (Phase 6) + native facts (Phase 2) into the §4 comment record. Marker triage is native (cross-language worklist)."
  - id: "7.2"
    name: implement-parse-invariant-applier
    action: "Create the native parse-invariant applier for agent-authored comment edits: after any comment-only edit, re-parse with the Phase 2 grammar and assert the code-node tree is byte-identical; if any code node changed, ABORT; guarantee deterministic + idempotent application."
    files: [crates/commenter-cat-engine/src/ops/apply/mod.rs, crates/commenter-cat-engine/src/ops/apply/parse_invariant.rs]
    depends_on: ["7.1"]
    parallel_safe: false
    validation: "cargo test -p commenter-cat-engine ops::apply::parse_invariant passes: a comment-only edit applies, an edit that would alter a code node aborts, and re-applying the same edit is idempotent"
    notes: "Idea §4a/§5: THE pillar that lets an external agent hold the write path. Parse-invariance is property-tested over arbitrary edits in Phase 10 (the load-bearing proptest). Reuses tree-sitter from 2.2."
  - id: "7.3"
    name: implement-write-protection-by-kind
    action: "Add write-protection by kind to the applier: directive (commenter-cat:*, # noqa, // eslint-disable, # type: ignore, // @ts-expect-error), shebang, and encoding-decl comments are parse-invariant yet behavior-bearing — the applier REFUSES to rewrite them unless an explicit allow_significant acknowledgment is passed, and flags the edit as significant."
    files: [crates/commenter-cat-engine/src/ops/apply/write_protection.rs, crates/commenter-cat-engine/src/ops/apply/mod.rs]
    depends_on: ["7.2"]
    parallel_safe: false
    validation: "cargo test -p commenter-cat-engine ops::apply::write_protection passes: editing a # type: ignore is refused without allow_significant and permitted (flagged) with it"
    notes: "Idea §4a/§5: parse-invariance proves the parser sees no change, NOT that behavior is unchanged. The kind taxonomy (§3) enforces this. Same module as 7.2 → sequential."
  - id: "7.4"
    name: implement-suppression-filter-up
    action: "Create the filter-up suppression pass at the normalization layer (never native directives in source): parse the commenter-cat:* directive grammar (disable-line, disable-next-line, disable=…enable region, disable-file) with target granularity = provider_rule_id | category | origin | all, flag suppressed findings with suppressed_by (kept in index, excluded from default views, revealed by --show-suppressed), and detect unused directives."
    files: [crates/commenter-cat-engine/src/ops/suppress/mod.rs, crates/commenter-cat-engine/src/ops/suppress/directives.rs]
    depends_on: ["7.1"]
    parallel_safe: true
    validation: "cargo test -p commenter-cat-engine ops::suppress passes: each directive scope suppresses correctly, a category-level target works, and an unused commenter-cat:disable is reported"
    notes: "Idea §5 Unified suppression: authoritative at our layer, one syntax for four tools. Suppressed = flagged not dropped (audit trail + unused-directive detection). commenter-cat:* classified kind=directive (never a finding target). Different subtree from 7.2/7.3 → parallel-safe."
  - id: "7.5"
    name: implement-baseline-file
    action: "Create the committed baseline file commenter-cat.baseline.toml (beside config, OUTSIDE the gitignored cache): sorted line-oriented entries (bound_symbol, cosmetic_fingerprint, rule) + optional reason/date, canonically ordered (lockfile-style), matched at Tier 2 (cosmetic, never fuzzy); implement `commenter-cat baseline accept` (snapshot current findings) and `commenter-cat baseline prune` (drop entries whose findings no longer occur)."
    files: [crates/commenter-cat-engine/src/ops/baseline/mod.rs, crates/commenter-cat-engine/src/ops/baseline/file_format.rs]
    depends_on: ["7.4"]
    parallel_safe: false
    validation: "cargo test -p commenter-cat-engine ops::baseline passes: accept snapshots findings deterministically, entries match at Tier 2, prune removes stale entries, and the file is canonically ordered"
    notes: "Idea §5 Baseline file: committed shared truth, outside .commenter-cat/. Tier-2 match (Phase 5) — never fuzzy. Records version+config_hash comparability (§5/§7). Inline directives + baseline are two inputs to ONE suppression pass (7.4)."
  - id: "7.6"
    name: implement-fix-and-tighten
    action: "Create `commenter-cat fix` / `commenter-cat tighten`: orchestrate provider autofixes by delegating to each tool's own --fix (ruff --fix, eslint --fix) where capabilities.supports_fix is true (Commenter-Cat never hand-applies a tool's edit), and route agent-authored comment edits through the parse-invariant applier; both deterministic + idempotent."
    files: [crates/commenter-cat-engine/src/ops/fix.rs, crates/commenter-cat-engine/src/ops/mod.rs]
    depends_on: ["7.3", "7.5"]
    parallel_safe: false
    validation: "cargo test -p commenter-cat-engine ops::fix passes: a supports_fix provider's autofix is delegated, an agent comment edit goes through parse-invariance, and a non-fixable finding is reported as agent_only"
    notes: "Idea §5 `commenter-cat fix`/`commenter-cat tighten`: provider autofixes delegated (each tool owns edit safety); native applier for agent edits. supports_fix drives delegation (capabilities, Phase 6)."
  - id: "7.7"
    name: implement-suppression-export
    action: "Create `commenter-cat suppressions export` (opt-in, one-way inverse of filter-up): materialize the suppression set into each tool's native directives (# noqa: D417, // eslint-disable-next-line, # shellcheck disable=…), written through the parse-invariant applier so source edits stay safe; the default flow never touches source."
    files: [crates/commenter-cat-engine/src/ops/suppress/export.rs, crates/commenter-cat-engine/src/ops/suppress/mod.rs]
    depends_on: ["7.6"]
    parallel_safe: false
    validation: "cargo test -p commenter-cat-engine ops::suppress::export passes: a suppression set materializes to correct per-tool native directives via the parse-invariant applier"
    notes: "Idea §5 Export mode (opt-in): for teams also running tools directly. Routes through the applier (7.2) → safe writes. Shares suppress/ module with 7.4 → sequential after it."
---

# Phase 7: Operations

## Goal
Wire the engine's verbs into working operations: the unified `commenter-cat check` (orchestrate providers + fuse native findings + marker triage), the parse-invariant safe-apply with write-protection by kind (the pillar that lets an external agent hold the write path), the filter-up suppression model with the committed baseline (one suppression syntax across four tools), provider-autofix delegation, and opt-in native-directive export. Depends on Phase 5 (identity, for Tier-2 baseline matching and suppression continuity) and Phase 6 (the provider layer). This phase realizes the "AI proposes, engine guarantees" division of labor.

## Tasks

### 7.1 — implement-commenter-cat-check-orchestration
- **Action:** `commenter-cat check` — run providers on cache-miss files, normalize + add native findings (blame-skew + marker triage), fuse into `findings[]`, persist.
- **Files:** `crates/commenter-cat-engine/src/ops/{check.rs,normalize.rs,triage.rs}`
- **Depends on:** none (within phase)
- **Validation:** provider + native + triage findings fuse correctly on a multi-language fixture.

### 7.2 — implement-parse-invariant-applier
- **Action:** Re-parse after a comment-only edit and assert the code-node tree is byte-identical; abort on any code-node change; deterministic + idempotent.
- **Files:** `crates/commenter-cat-engine/src/ops/apply/{mod.rs,parse_invariant.rs}`
- **Depends on:** 7.1
- **Validation:** comment edit applies; code-altering edit aborts; re-apply idempotent.
- Property-tested over arbitrary edits in Phase 10.

### 7.3 — implement-write-protection-by-kind
- **Action:** Refuse to rewrite `directive`/`shebang`/`encoding-decl` comments without `allow_significant`; flag when permitted.
- **Files:** `crates/commenter-cat-engine/src/ops/apply/write_protection.rs`, `crates/commenter-cat-engine/src/ops/apply/mod.rs`
- **Depends on:** 7.2 (shares `apply/`)
- **Validation:** `# type: ignore` edit refused without ack, permitted+flagged with it.

### 7.4 — implement-suppression-filter-up
- **Action:** Filter-up suppression at the normalization layer: `commenter-cat:*` directive grammar, target granularity, `suppressed_by` flagging, unused-directive detection.
- **Files:** `crates/commenter-cat-engine/src/ops/suppress/{mod.rs,directives.rs}`
- **Depends on:** 7.1 (parallel-safe with 7.2/7.3 — different subtree)
- **Validation:** each scope + category target suppresses; unused directive reported.

### 7.5 — implement-baseline-file
- **Action:** Committed `commenter-cat.baseline.toml` (Tier-2 matched, canonically ordered) + `commenter-cat baseline accept`/`prune`.
- **Files:** `crates/commenter-cat-engine/src/ops/baseline/{mod.rs,file_format.rs}`
- **Depends on:** 7.4
- **Validation:** accept snapshots deterministically; Tier-2 match; prune removes stale; canonical order.

### 7.6 — implement-fix-and-tighten
- **Action:** `commenter-cat fix`/`commenter-cat tighten` — delegate provider autofixes (supports_fix), route agent edits through the applier.
- **Files:** `crates/commenter-cat-engine/src/ops/fix.rs`, `crates/commenter-cat-engine/src/ops/mod.rs`
- **Depends on:** 7.3, 7.5
- **Validation:** autofix delegated; agent edit parse-invariant; non-fixable → agent_only.

### 7.7 — implement-suppression-export
- **Action:** Opt-in `commenter-cat suppressions export` to native directives via the parse-invariant applier.
- **Files:** `crates/commenter-cat-engine/src/ops/suppress/export.rs`, `crates/commenter-cat-engine/src/ops/suppress/mod.rs`
- **Depends on:** 7.6
- **Validation:** suppression set materializes to correct per-tool directives via the applier.

## Phase Validation
`cargo test -p commenter-cat-engine ops::` passes across check, apply (parse_invariant + write_protection), suppress (+ export), and baseline; an end-to-end run on a fixture repo produces unified findings, suppresses via both inline directives and a committed baseline in one pass, applies an agent-authored comment edit under the parse-invariance guarantee, refuses a behavior-bearing-kind edit without acknowledgment, and delegates a provider autofix.
