---
plan_slug: 2026-06-15-build-commenter-cat-engine
created_at: 2026-06-15T00:00:00Z
created_by: planner-skill
total_phases: 10
total_tasks: 52
estimated_files_touched: 120
files:
  - { path: 1-Objective.md,                                  summary: "Goal and success criteria" }
  - { path: 2-Scope.md,                                      summary: "In/out scope, constraints" }
  - { path: 3-Phases/phase-1-foundation.md,                  summary: "Cargo workspace, errors, versioned contracts, config" }
  - { path: 3-Phases/phase-2-native-substrate.md,            summary: "Walk, tree-sitter extract, map, markers, git, rot candidates" }
  - { path: 3-Phases/phase-3-finding-model.md,               summary: "Canonical Finding, severity, coordinates, dedup" }
  - { path: 3-Phases/phase-4-storage.md,                     summary: "Two-layer SQLite, FTS5, ONNX embeddings, sqlite-vec, rebuild" }
  - { path: 3-Phases/phase-5-identity.md,                    summary: "Cosmetic fingerprint, composite identity, tiered matching" }
  - { path: 3-Phases/phase-6-providers.md,                   summary: "RuleProvider trait, manifests, eslint native, pinning, doctor" }
  - { path: 3-Phases/phase-7-operations.md,                  summary: "cf check, parse-invariant apply, suppression, baseline, fix" }
  - { path: 3-Phases/phase-8-interfaces.md,                  summary: "CLI, renderers, MCP surface, token economy, round-trip" }
  - { path: 3-Phases/phase-9-adjacent-integrations.md,       summary: "comment-to-issue, git hooks, CI integration" }
  - { path: 3-Phases/phase-10-distribution-and-tests.md,     summary: "Property/golden/contract/integration tests, OS-matrix CI, cargo-dist" }
  - { path: 4-Context.md,                                    summary: "Detected stack, patterns, rules, architecture diagram" }
  - { path: 5-Validation.md,                                 summary: "Per-phase + integration checks and Definition of Done" }
  - { path: 6-Risks.md,                                      summary: "Risks, open questions, assumptions, rollback" }
  - { path: 7-Execution.md,                                  summary: "Dependency DAG, critical path, parallel groups, atomic-claim" }
  - { path: STATUS.md,                                       summary: "Live per-task state (only mutable plan file)" }
  - { path: CHANGELOG.md,                                    summary: "Append-only re-plan audit" }
reading_order:
  orchestrator: [0-Index.md, 7-Execution.md, 4-Context.md, 3-Phases]
  sequential:   [0-Index.md, 1-Objective.md, 2-Scope.md, 4-Context.md, 3-Phases]
---

# Build Commenter-Cat — the Rust comment-intelligence engine (`cf`)

This plan builds **Commenter-Cat** (`cf`): a deterministic, multi-language comment-intelligence layer — a Rust orchestrator that walks a directory, extracts comments with tree-sitter, maps each comment to the code it annotates, delegates per-language rule-checking to best-in-class external tools (ruff, eslint+jsdoc/tsdoc, shellcheck, gitleaks), normalizes their findings into one model, enriches with git, indexes everything in a per-project two-layer SQLite cache (keyword + vector search), and exposes the whole thing as an **agent-facing MCP surface** that a coding agent drives in a live session — the product (Idea §4a). The plan is greenfield: the repo currently holds only `Docs/Idea.md`, `LICENSE`, and `.gitignore`. Phases sequence by hard build dependency, not by rollout — per the Idea's build principle (§0), every capability is in scope as one complete system; there is no MVP, beta, or deferred feature. Start at `7-Execution.md` (orchestrator) or `1-Objective.md` (sequential).

## Files

- [1-Objective.md](1-Objective.md) — Goal and success criteria
- [2-Scope.md](2-Scope.md) — In/out scope, constraints
- [3-Phases/phase-1-foundation.md](3-Phases/phase-1-foundation.md) — Cargo workspace, errors, versioned contracts, config
- [3-Phases/phase-2-native-substrate.md](3-Phases/phase-2-native-substrate.md) — Walk, tree-sitter extract, map, markers, git, rot candidates
- [3-Phases/phase-3-finding-model.md](3-Phases/phase-3-finding-model.md) — Canonical Finding, severity, coordinates, dedup
- [3-Phases/phase-4-storage.md](3-Phases/phase-4-storage.md) — Two-layer SQLite, FTS5, ONNX embeddings, sqlite-vec, rebuild
- [3-Phases/phase-5-identity.md](3-Phases/phase-5-identity.md) — Cosmetic fingerprint, composite identity, tiered matching
- [3-Phases/phase-6-providers.md](3-Phases/phase-6-providers.md) — RuleProvider trait, manifests, eslint native, pinning, doctor
- [3-Phases/phase-7-operations.md](3-Phases/phase-7-operations.md) — cf check, parse-invariant apply, suppression, baseline, fix
- [3-Phases/phase-8-interfaces.md](3-Phases/phase-8-interfaces.md) — CLI, renderers, MCP surface, token economy, round-trip
- [3-Phases/phase-9-adjacent-integrations.md](3-Phases/phase-9-adjacent-integrations.md) — comment-to-issue, git hooks, CI integration
- [3-Phases/phase-10-distribution-and-tests.md](3-Phases/phase-10-distribution-and-tests.md) — Tests, OS-matrix CI, cargo-dist
- [4-Context.md](4-Context.md) — Detected stack, patterns, rules, architecture diagram
- [5-Validation.md](5-Validation.md) — Per-phase + integration checks and Definition of Done
- [6-Risks.md](6-Risks.md) — Risks, open questions, assumptions, rollback
- [7-Execution.md](7-Execution.md) — Dependency DAG, critical path, parallel groups, atomic-claim
- [STATUS.md](STATUS.md) — Live per-task state (only mutable plan file)
- [CHANGELOG.md](CHANGELOG.md) — Append-only re-plan audit

## Consumption

- **Orchestrator agent:** read `7-Execution.md` for the DAG, then dispatch phase/task files to workers per `STATUS.md`. Apply the atomic-claim protocol. Phases 2 and 3 parallelize after Phase 1; Phases 5 and 6 parallelize after Phase 4; Phases 9 and 10 parallelize after Phase 8.
- **Sequential agent:** read `1-Objective.md` → `2-Scope.md` → `4-Context.md`, then walk `3-Phases/` in order, updating `STATUS.md` per task.
