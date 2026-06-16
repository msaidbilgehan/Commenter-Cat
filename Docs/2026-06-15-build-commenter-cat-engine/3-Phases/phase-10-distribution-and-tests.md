---
plan_slug: 2026-06-15-build-commenter-cat-engine
phase: 10
name: distribution-and-tests
goal: "Prove the two product promises (determinism + safe-write) with the layered test suite, and ship: property/golden/contract/integration/repro/MCP/perf tests, OS-matrix CI, and cargo-dist with the per-platform native-artifact matrix."
depends_on_phases: [8]
parallel_safe_with_phases: [9]
tasks:
  - id: "10.1"
    name: write-property-tests
    action: "Create the load-bearing proptest suite: parse-invariance over arbitrary comment edits (code-node tree byte-identical), apply_edit idempotence, identity stability (a cosmetic edit preserves Tier-2, a marker escalation breaks it), and findings-ordering determinism; plus unit tests for UTF-16↔byte conversion and JSONPath extraction."
    files: [crates/commenter-cat-engine/tests/prop_parse_invariance.rs, crates/commenter-cat-engine/tests/prop_identity.rs, crates/commenter-cat-core/tests/prop_coordinates.rs]
    depends_on: []
    parallel_safe: true
    validation: "cargo test -p commenter-cat-engine --test prop_parse_invariance --test prop_identity && cargo test -p commenter-cat-core --test prop_coordinates all pass"
    notes: "Idea §11: 'the load-bearing ones' — safety guarantees are property-tested, not example-tested. proptest (§11). These prove the two promises. Reuses applier (7.2), identity (Phase 5), coordinates (3.3)."
  - id: "10.2"
    name: write-golden-and-contract-tests
    action: "Create the golden-file (insta) tests for per-grammar extraction + mapping (bound_symbol, kind, ranges) and per-provider JSON→Finding[] (recorded, version-decoupled fixtures), plus the shared provider-contract harness asserting every adapter (manifest + native, including dogfooded built-ins) honors the §5 contract: JSON-only, coordinate mapping, run-state SUCCESS/EMPTY/PARTIAL/SKIPPED, failure ≠ zero."
    files: [crates/commenter-cat-engine/tests/golden_extraction.rs, crates/commenter-cat-engine/tests/provider_contract.rs, crates/commenter-cat-engine/tests/fixtures/README.md]
    depends_on: ["10.1"]
    parallel_safe: false
    validation: "cargo test -p commenter-cat-engine --test golden_extraction --test provider_contract pass; every built-in provider passes the shared contract harness"
    notes: "Idea §11: insta golden-files + provider contract. Recorded fixtures decouple from live tool versions. The contract harness runs against manifest + native + dogfooded built-ins (Phase 6)."
  - id: "10.3"
    name: write-integration-and-mcp-tests
    action: "Create integration tests on fixture repos with pinned providers (end-to-end commenter-cat check: unified findings, suppression, baseline, and the graceful-degradation path provider-absent→SKIPPED explicitly tested) and MCP agent-contract tests (token budget honored via limit/cursor/truncated, the apply_edit round-trip returns re-checked findings, write-protection-by-kind refusal); mock only at seams (network/clock) — never the parser or DB."
    files: [crates/commenter-cat-engine/tests/integration_check.rs, crates/commenter-cat-engine/tests/mcp_contract.rs, crates/commenter-cat-engine/tests/fixtures/repos/.gitkeep]
    depends_on: ["10.2"]
    parallel_safe: false
    validation: "cargo test -p commenter-cat-engine --test integration_check --test mcp_contract pass, including the provider-absent SKIPPED degradation path and the MCP token-budget + round-trip + write-protection assertions"
    notes: "Idea §11 Integration + MCP surface layers. Real tree-sitter + real SQLite (§11 principle). Graceful-degradation path explicitly required. Validates Phase 7 ops + Phase 8 surface end to end."
  - id: "10.4"
    name: configure-distribution-and-ci
    action: "Configure cargo-dist + GitHub Releases for prebuilt binaries (cargo binstall) across the five tier-1 targets (x86_64/aarch64 linux gnu + musl, aarch64/x86_64 darwin, x86_64 windows msvc), attach the version-matched per-(os,arch) sqlite-vec + ONNX artifacts (a missing artifact fails the release), and set up the OS-matrix CI (Linux/macOS/Windows) running the full suite + the reproducibility check (identical comparability key → byte-identical report across two runs and across the OS matrix) + criterion perf-regression benches with commenter-cat check --stats."
    files: [.github/workflows/ci.yml, .github/workflows/release.yml, dist-workspace.toml, crates/commenter-cat-engine/benches/native_pass.rs]
    depends_on: ["10.3"]
    parallel_safe: false
    validation: "the CI workflow runs the full test suite on all three OSes and the release workflow builds all five targets with their native artifacts; the reproducibility job asserts byte-identical reports across two runs"
    notes: "Idea §10 Distribution + §11 reproducibility/perf. cargo-dist + binstall (§10). Native-artifact matrix is 'the real packaging work'; missing artifact fails release, never ships degraded. CRLF/Windows explicitly tested (§10/§11). criterion benches assert the per-stage budget shape (§6)."
---

# Phase 10: Distribution and Tests

## Goal
Prove the product's two promises — **determinism** (same input → same findings) and **safe-write** (code byte-identical + behavior preserved) — and ship the binary. The safety guarantees are property-tested, not asserted; the per-grammar extraction and per-provider normalization are golden-file fixtures; every adapter passes a shared contract harness; integration and MCP-surface tests exercise the end-to-end loop including the graceful-degradation path; and `cargo-dist` plus the OS-matrix CI deliver reproducible prebuilt binaries with their version-matched native artifacts. Depends on Phase 8 (the full surface under test). Runs in parallel with Phase 9 (adjacent integrations).

## Tasks

### 10.1 — write-property-tests
- **Action:** proptest suite — parse-invariance over arbitrary edits, apply_edit idempotence, identity stability, findings-ordering determinism; + UTF-16↔byte and JSONPath unit tests.
- **Files:** `crates/commenter-cat-engine/tests/{prop_parse_invariance.rs,prop_identity.rs}`, `crates/commenter-cat-core/tests/prop_coordinates.rs`
- **Depends on:** none (within phase)
- **Validation:** all three property-test binaries pass.
- These are "the load-bearing ones" — they prove the two promises.

### 10.2 — write-golden-and-contract-tests
- **Action:** insta golden-files (extraction + mapping, provider JSON→Finding[]) + shared provider-contract harness (JSON-only, coordinates, run-state, failure≠zero).
- **Files:** `crates/commenter-cat-engine/tests/{golden_extraction.rs,provider_contract.rs}`, `crates/commenter-cat-engine/tests/fixtures/README.md`
- **Depends on:** 10.1
- **Validation:** golden + contract pass; every built-in passes the harness.

### 10.3 — write-integration-and-mcp-tests
- **Action:** Fixture-repo integration (end-to-end check + suppression + baseline + provider-absent→SKIPPED) + MCP agent-contract (token budget, round-trip, write-protection refusal); mock only at seams.
- **Files:** `crates/commenter-cat-engine/tests/{integration_check.rs,mcp_contract.rs}`, `crates/commenter-cat-engine/tests/fixtures/repos/.gitkeep`
- **Depends on:** 10.2
- **Validation:** both pass, including the SKIPPED degradation path and the MCP assertions.

### 10.4 — configure-distribution-and-ci
- **Action:** cargo-dist + Releases (five targets, version-matched sqlite-vec/ONNX artifacts, missing-artifact-fails-release) + OS-matrix CI (full suite + reproducibility + criterion perf benches).
- **Files:** `.github/workflows/{ci.yml,release.yml}`, `dist-workspace.toml`, `crates/commenter-cat-engine/benches/native_pass.rs`
- **Depends on:** 10.3
- **Validation:** CI runs on three OSes; release builds five targets with artifacts; reproducibility job asserts byte-identical reports.

## Phase Validation
The full suite — `cargo test --workspace` (unit + property + golden + contract + integration + MCP) and the criterion benches — passes on the Linux/macOS/Windows matrix; the reproducibility job produces byte-identical reports for an identical `(tree_hash, commenter_cat_ruleset_version, provider_version, config_hash)` across two runs and across the OS matrix; and the release workflow builds all five tier-1 targets with their version-matched native artifacts, failing if any artifact is missing.
