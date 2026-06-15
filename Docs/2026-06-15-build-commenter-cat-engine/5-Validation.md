---
plan_slug: 2026-06-15-build-commenter-cat-engine
section: validation
validation_levels: [phase, integration, acceptance]
---

# Validation

## Per-Phase Validation

- **Phase 1 (foundation):** `cargo build --workspace && cargo test -p cf-core && cargo clippy --workspace -- -D warnings` — workspace compiles with error, version, and config modules exported from `cf-core`.
- **Phase 2 (native substrate):** `cargo test -p cf-engine` for walk/extract/map/markers/git/rot — a full §4 comment record (minus `findings[]`) is produced for a multi-language fixture tree.
- **Phase 3 (finding model):** `cargo test -p cf-core finding::` — provider- and native-shaped findings normalize, severity-resolve, coordinate-convert, dedup, and emit in canonical order deterministically.
- **Phase 4 (storage):** `cargo test -p cf-engine storage:: search:: embed::` — a fixture scan persists into the two-layer cache, answers a hybrid query, and survives an `index.db` bump via byte-identical rebuild from `inputs.db` with no provider re-run / re-embed.
- **Phase 5 (identity):** `cargo test -p cf-core identity:: && cargo test -p cf-engine identity:: storage::identity_store` — the same comment matches across whitespace edit (Tier 2), relocation (Tier 3), and reword (Tier 4 for issue continuity only), with fuzzy provably excluded from suppression.
- **Phase 6 (providers):** `cargo test -p cf-engine provider::` — the four built-ins run JSON-only, normalize to canonical `Finding`s, resolve run-state correctly on absent/crashed providers, and `cf doctor` reports a coherent version+config comparison.
- **Phase 7 (operations):** `cargo test -p cf-engine ops::` — unified findings + suppression (inline + baseline in one pass) + an agent comment edit under parse-invariance + a refused behavior-bearing-kind edit + a delegated provider autofix.
- **Phase 8 (interfaces):** `cargo test -p cf-cli cli:: && cargo test -p cf-engine render:: mcp:: surface::` — a fixture session driven through CLI and MCP: token-budgeted ranked `query`/`check`, `context` drill, and an `apply_edit` returning re-checked findings in one call.
- **Phase 9 (adjacent):** `cargo test -p cf-engine issues:: hooks:: ci::` — two-layer CI cache restore (reusing provider results across a CF-version bump), baseline diff, degraded verdict on PARTIAL, and idempotent comment-to-issue + non-blocking hooks.
- **Phase 10 (distribution + tests):** `cargo test --workspace` (unit + property + golden + contract + integration + MCP) + criterion benches pass on the OS matrix; the reproducibility job yields byte-identical reports; the release workflow builds all five targets with their native artifacts.

## Integration Tests

- **End-to-end live-session loop:** drive the six MCP primitives through a complete find→understand→rule-check→update→re-check cycle on a fixture repo; assert `apply_edit`/`remove` return re-checked findings inline (loop is one tool-call deep).
- **Cross-language unified `check`:** on a fixture tree containing Python, TS, JS, and Shell files (plus a planted secret and a documented-param drift), assert one normalized report fuses ruff + eslint + shellcheck + gitleaks + native findings with correct categories, severities, dedup, and canonical order.
- **Safe-write property proof:** `proptest` generates arbitrary comment edits; assert the code-node tree is byte-identical after every accepted edit, `apply_edit` is idempotent, and any edit that would mutate a code node aborts.
- **Write-protection-by-kind:** assert editing a `# type: ignore` / `// eslint-disable` / shebang is refused without `allow_significant` and flagged-as-significant when permitted.
- **Suppression in one pass:** assert an inline `cf:disable` and a committed baseline entry suppress the same finding through one normalization pass (Tier-2 match), suppressed findings are flagged not dropped, and an unused directive is reported.
- **Graceful degradation:** with a provider binary absent, assert the run-state is `SKIPPED`, native mapping/search/candidates still run, and the verdict is non-fatal (`on_missing = warn`).
- **Reproducibility across runs + OS:** two runs with an identical `(tree_hash, cf_ruleset_version, provider_version, config_hash)` produce byte-identical JSONL/SARIF, on each of Linux/macOS/Windows.
- **Two-layer CI cache across a CF upgrade:** assert an `inputs.db` hit (keyed on provider/config, not CF version) skips provider re-runs after a simulated CF-version bump, while an `index.db` miss re-derives from `inputs.db` rather than cold-scanning.

## Definition of Done
The plan is complete when ALL of the following are true:

- [ ] Every task in `STATUS.md` has state `done` or `rejected`.
- [ ] Every per-phase validation passes.
- [ ] Every integration test passes.
- [ ] No `failed` or `blocked` task remains unresolved.
- [ ] **The agent loop works end to end** — the six MCP primitives (1:1 with CLI verbs) drive a full find→update→re-check cycle one tool-call deep, with inline re-checked findings on write.
- [ ] **Unified cross-language findings** — `cf check` normalizes all four providers + native facts into the canonical `Finding` schema with lossless `provider_rule_id`, category-anchored 4-level severity, and deduped canonical ordering.
- [ ] **Deterministic comment→code mapping** with zero string-vs-comment false positives, verified by per-grammar golden files.
- [ ] **Safe-write proven** — parse-invariance + write-protection-by-kind are property-tested over arbitrary edits.
- [ ] **Reproducibility holds** — identical comparability key → byte-identical reports across two runs and the Linux/macOS/Windows matrix.
- [ ] **Two-layer cache + manifest-first** — `inputs.db` survives schema bumps, `index.db` rebuilds; the three single-binary built-ins ship as dogfooded manifests.
- [ ] **Token economy enforced** — no agent-facing return is unbounded; truncation is always labeled with a cursor.
- [ ] **Distribution** — `cargo install`/`binstall` across the five tier-1 targets with version-matched per-platform sqlite-vec + ONNX artifacts; the full layered suite passes on the OS matrix.
