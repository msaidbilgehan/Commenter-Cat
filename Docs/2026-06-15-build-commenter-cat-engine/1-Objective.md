---
plan_slug: 2026-06-15-build-commenter-cat-engine
section: objective
---

# Objective

Build **Commenter-Cat** (`commenter-cat`) as one complete system: a Rust orchestrator that owns the comment-intelligence *substrate* (walk · extract · map · enrich · normalize · index · search · surface), **delegates per-language rule content** to best-in-class external providers (ruff, eslint+jsdoc/tsdoc, shellcheck, gitleaks) through swappable adapters, and exposes the result as an **agent-drivable MCP surface** — the product — on which a coding agent runs the live-session loop *find → understand → rule-check → safely-update → re-check* across Python, TypeScript, JavaScript, and Shell. The engine hosts neither an LLM nor a reimplemented linter; it unifies specialists' output and hands the consuming agent the judgment-shaped work, guarded by a parse-invariant safe-write path.

## Success Criteria

- **The agent loop works end to end (the product, Idea §4a):** the six MCP primitives (`query`, `context`, `check`, `candidates`, `apply_edit`, `remove`), 1:1 with CLI verbs, drive a full find→update→re-check cycle one tool-call deep, with `apply_edit`/`remove` returning re-checked findings inline.
- **Unified cross-language findings:** `commenter-cat check` runs the four providers (JSON-only, batched, parallel), normalizes every output plus native facts into one canonical `Finding` schema with lossless `provider_rule_id`, category-anchored 4-level severity, and deduped canonical ordering.
- **Deterministic comment→code mapping:** every comment binds to `bound_symbol`/`bound_node_range` by the three deterministic rules (PEP 257 / JSDoc adjacency / line geometry), with zero string-vs-comment false positives.
- **Safe-write is proven, not asserted:** parse-invariance (code-node tree byte-identical after any comment edit) and write-protection-by-kind are **property-tested** over arbitrary edits; a behavior-bearing-kind edit is refused without `allow_significant`.
- **Reproducibility holds:** an identical `(tree_hash, commenter_cat_ruleset_version, provider_version, config_hash)` yields byte-identical reports across two runs and across the Linux/macOS/Windows matrix.
- **Two-layer cache + manifest-first platform:** content-addressed `inputs.db` survives schema bumps while derived `index.db` rebuilds (never migrates); ≥80–90% of provider integrations are expressible as declarative manifests with no compiled code.
- **Token economy enforced:** no agent-facing return is a firehose — every result is ranked, budget-bounded, labeled-when-truncated, and drillable via `context`.
- **Distribution:** `cargo install` / `cargo binstall` across five tier-1 targets with version-matched per-platform sqlite-vec + ONNX artifacts; the full layered test suite passes on the OS matrix.

## Non-goals

- **No LLM and no reinvented linter in the engine flow** — the consuming agent brings judgment; mature tools bring rule content (Idea §1, §13).
- **No semantic-rot detection as an engine feature** — the engine supplies evidence (`context`) + candidates (blame-skew) + safe apply; the agent judges (Idea §13).
- **No new languages beyond Python/TypeScript/JavaScript/Shell** in this plan — more grammars are low-cost future additions, out of defined scope (Idea §3).
- **No phased rollout / MVP / 0.x** — phases here are a build-order DAG, not a release plan; the product ships at 1.0 as one complete system (Idea §0).
