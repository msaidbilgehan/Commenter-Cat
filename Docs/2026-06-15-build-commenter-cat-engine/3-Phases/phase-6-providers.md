---
plan_slug: 2026-06-15-build-commenter-cat-engine
phase: 6
name: providers
goal: "Build the manifest-first provider layer: the RuleProvider trait + invocation contract, the run-state machine, the JSONPath manifest provider, declarative severity/category maps, the dogfooded built-in manifests, the eslint Tier-2 native provider with Node tiers, and provider pinning + config-fingerprint + doctor."
depends_on_phases: [3, 4]
parallel_safe_with_phases: [5]
tasks:
  - id: "6.1"
    name: define-ruleprovider-trait
    action: "Create the RuleProvider trait and invocation contract in commenter-cat-engine per Idea §5: one subprocess per provider batched over the cache-miss file set, parallel across providers, per-provider timeout, JSON-only I/O, file vs project scope, two-layer file discovery effective_scope = provider_filter(commenter_cat_scope), and canonical finding order (file, line, col, provider_rule_id)."
    files: [crates/commenter-cat-engine/src/provider/mod.rs, crates/commenter-cat-engine/src/provider/contract.rs, crates/commenter-cat-engine/src/provider/discovery.rs]
    depends_on: []
    parallel_safe: true
    validation: "cargo test -p commenter-cat-engine provider::contract passes: commenter_cat_scope authority (a commenter-cat-excluded file is never passed) and provider veto (a tool-config-ignored file is skipped) both hold"
    notes: "Idea §5 invocation contract + file discovery. commenter_cat_scope authoritative; provider only vetoes within it. JSON mode only — never text scraping. Trait is the seam mocked in tests, never the parser/DB (§11)."
  - id: "6.2"
    name: implement-run-state-machine
    action: "Create the provider run-state machine: SUCCESS (ran, findings), EMPTY (ran, 0 findings), PARTIAL (crash/timeout/malformed JSON — findings unavailable, not zero), SKIPPED (provider absent / language off); exit code is NOT the signal (parsed JSON = ran); a PARTIAL provider sets baseline_state=PARTIAL; default on_error=warn, --strict makes PARTIAL fatal."
    files: [crates/commenter-cat-engine/src/provider/run_state.rs, crates/commenter-cat-engine/src/provider/mod.rs]
    depends_on: ["6.1"]
    parallel_safe: false
    validation: "cargo test -p commenter-cat-engine provider::run_state passes: malformed JSON → PARTIAL (not EMPTY), absent binary → SKIPPED, nonzero-exit-with-valid-JSON → SUCCESS"
    notes: "Idea §5 run-state machine: 'provider failure ≠ zero findings.' This is the correctness pillar for diff confidence (§7). Same module as 6.1 → sequential."
  - id: "6.3"
    name: implement-manifest-provider
    action: "Create the ManifestProvider (Tier 1) consuming a manifest_version=1 TOML adapter: command/format/scope, [[findings]] with RFC 9535 JSONPath extraction (pinned dialect) for iterate/native_rule_id/message/file/line/column, and a built-in generic SARIF mapper for format=\"sarif\"; forbid arbitrary code in manifests."
    files: [crates/commenter-cat-engine/src/provider/manifest/mod.rs, crates/commenter-cat-engine/src/provider/manifest/jsonpath.rs, crates/commenter-cat-engine/src/provider/manifest/sarif.rs]
    depends_on: ["6.1"]
    parallel_safe: true
    validation: "cargo test -p commenter-cat-engine provider::manifest passes: nested JSONPath extraction yields correct Findings, and a SARIF document maps via the generic mapper with no field-paths"
    notes: "Idea §5 Tier 1 manifest providers (~80–90% of integrations). RFC 9535 pinned (§14 picks the concrete lib). NO embedded JS/Python/transform strings — that is the threshold for Tier 2. Different subtree from 6.2 → parallel-safe."
  - id: "6.4"
    name: implement-manifest-mapping-tables
    action: "Add the declarative mapping layer to ManifestProvider: [severity_map] and [category_map] tables bridging native values to the canonical Finding, plus the [capabilities] block (file_scoped/project_scoped, supports_fix, supports_incremental, supports_sarif, coordinate_system) that drives the orchestrator generically."
    files: [crates/commenter-cat-engine/src/provider/manifest/mapping.rs, crates/commenter-cat-engine/src/provider/manifest/capabilities.rs]
    depends_on: ["6.3"]
    parallel_safe: false
    validation: "cargo test -p commenter-cat-engine provider::manifest::mapping passes: severity_map/category_map produce canonical severity+category, and [capabilities] coordinate_system routes to the correct conversion from Phase 3"
    notes: "Idea §5: mapping = declarative tables, no code. [capabilities] is the single declarative source for §5 invocation behavior — orchestrator reasons from declared capabilities, not hardcoded per-tool knowledge."
  - id: "6.5"
    name: author-builtin-manifests
    action: "Author the dogfooded built-in manifests loaded exactly like user adapters: ruff.manifest.toml (D/ERA/TD/D417 subset), shellcheck.manifest.toml (shebang/header/directive), gitleaks.manifest.toml (secrets) — each filtered to the comment domain (curated rule subset, never full code-lint output)."
    files: [crates/commenter-cat-engine/assets/providers/ruff.manifest.toml, crates/commenter-cat-engine/assets/providers/shellcheck.manifest.toml, crates/commenter-cat-engine/assets/providers/gitleaks.manifest.toml, crates/commenter-cat-engine/src/provider/builtins.rs]
    depends_on: ["6.4"]
    parallel_safe: false
    validation: "cargo test -p commenter-cat-engine provider::builtins passes: each built-in manifest loads via ManifestProvider and a recorded fixture (ruff/shellcheck/gitleaks JSON) maps to expected Findings"
    notes: "Idea §5 Dogfooding rule: built-ins ARE manifests; every built-in must be a manifest unless a documented technical limitation requires native. Comment-domain filter only (ruff D/ERA/TD, etc.). gitleaks always critical."
  - id: "6.6"
    name: implement-eslint-native-provider
    action: "Create the eslint Tier-2 native RuleProvider (the documented exception): drive eslint + eslint-plugin-jsdoc (+ tsdoc) via JSON mode, manage TS-program/project scope, convert UTF-16 columns via Phase 3, ingest the curated comment-rule subset (jsdoc/check-param-names, require-jsdoc, no-warning-comments, style), and implement the two Node tiers — default SEMI_HERMETIC (system Node ≥ min, pinned plugin tree) and --hermetic HERMETIC (fetched pinned Node)."
    files: [crates/commenter-cat-engine/src/provider/native/eslint.rs, crates/commenter-cat-engine/src/provider/native/node_runtime.rs, crates/commenter-cat-engine/src/provider/native/mod.rs]
    depends_on: ["6.4"]
    parallel_safe: true
    validation: "cargo test -p commenter-cat-engine provider::native::eslint passes against a recorded eslint JSON fixture: jsdoc findings map with UTF-16→byte columns corrected; node_runtime reports the correct reproducibility_level"
    notes: "Idea §5 Tier 2 + Node runtime tiers + §10: eslint is the lone Tier-2 native burden, fetched only when JS/TS in scope. reproducibility_level recorded in run metadata. Different subtree from 6.5 → parallel-safe."
  - id: "6.7"
    name: implement-provider-management
    action: "Create provider pinning + reproducibility + doctor: fetch/cache pinned provider versions at ~/.cache/commenter-cat/providers/ (single-binary tools cleanly; eslint pins a full lockfile tree), pin by version/hash, record per-provider version + config_hash (over resolved effective config via eslint --print-config / ruff --show-settings / tsc --showConfig) in provider_state, default pinned mode with --system-tools escape hatch, and `commenter-cat doctor`/`commenter-cat doctor --providers` validating version AND config against the baseline."
    files: [crates/commenter-cat-engine/src/provider/management/mod.rs, crates/commenter-cat-engine/src/provider/management/pinning.rs, crates/commenter-cat-engine/src/provider/management/config_fingerprint.rs, crates/commenter-cat-engine/src/provider/management/doctor.rs]
    depends_on: ["6.5", "6.6"]
    parallel_safe: false
    validation: "cargo test -p commenter-cat-engine provider::management passes: config_hash over a resolved effective config detects a settings change, and doctor flags a version skew + config-differs while honoring pinned"
    notes: "Idea §5 Provider management & reproducibility: comparability key (commenter_cat_ruleset_version, provider_version, config_hash). Repo owns external-linter config; Commenter-Cat owns native-provider config. Baselines updated only under pinned tools. Depends on both manifest (6.5) and eslint (6.6) being present."
---

# Phase 6: Providers

## Goal
Build the manifest-first provider layer that makes Commenter-Cat a platform rather than a fixed bundle of four analyzers. The `RuleProvider` trait + invocation contract define how `commenter-cat` drives any tool; the run-state machine guarantees "provider failure ≠ zero findings"; the JSONPath `ManifestProvider` (Tier 1) covers ~80–90% of integrations declaratively; the dogfooded built-in manifests prove the design; the eslint native provider (Tier 2) is the lone documented exception with its Node-runtime tiers; and provider management delivers pinning, config-fingerprinting, and `commenter-cat doctor`. Depends on Phase 3 (the `Finding` schema + coordinate conversion) and Phase 4 (the provider-result cache). Runs in parallel with Phase 5 (identity).

## Tasks

### 6.1 — define-ruleprovider-trait
- **Action:** `RuleProvider` trait + invocation contract (subprocess, batched, parallel, timeout, JSON-only, scope, `provider_filter(commenter_cat_scope)`, canonical order).
- **Files:** `crates/commenter-cat-engine/src/provider/{mod.rs,contract.rs,discovery.rs}`
- **Depends on:** none (within phase)
- **Validation:** commenter_cat_scope authority + provider veto both hold.

### 6.2 — implement-run-state-machine
- **Action:** SUCCESS/EMPTY/PARTIAL/SKIPPED; exit code is not the signal; `--strict` makes PARTIAL fatal.
- **Files:** `crates/commenter-cat-engine/src/provider/run_state.rs`, `crates/commenter-cat-engine/src/provider/mod.rs`
- **Depends on:** 6.1
- **Validation:** malformed JSON → PARTIAL; absent binary → SKIPPED; nonzero-exit-valid-JSON → SUCCESS.

### 6.3 — implement-manifest-provider
- **Action:** Tier-1 `ManifestProvider` with RFC 9535 JSONPath extraction + generic SARIF mapper; no code in manifests.
- **Files:** `crates/commenter-cat-engine/src/provider/manifest/{mod.rs,jsonpath.rs,sarif.rs}`
- **Depends on:** 6.1 (parallel-safe with 6.2 — different subtree)
- **Validation:** nested JSONPath extraction; SARIF via generic mapper.

### 6.4 — implement-manifest-mapping-tables
- **Action:** `[severity_map]`, `[category_map]`, `[capabilities]` declarative layer driving the orchestrator generically.
- **Files:** `crates/commenter-cat-engine/src/provider/manifest/{mapping.rs,capabilities.rs}`
- **Depends on:** 6.3
- **Validation:** maps produce canonical severity+category; `coordinate_system` routes to Phase 3 conversion.

### 6.5 — author-builtin-manifests
- **Action:** Dogfooded `ruff`/`shellcheck`/`gitleaks` manifests + loader, comment-domain filtered.
- **Files:** `crates/commenter-cat-engine/assets/providers/{ruff,shellcheck,gitleaks}.manifest.toml`, `crates/commenter-cat-engine/src/provider/builtins.rs`
- **Depends on:** 6.4
- **Validation:** each loads via ManifestProvider; recorded fixtures map to expected Findings.

### 6.6 — implement-eslint-native-provider
- **Action:** Tier-2 eslint native provider (jsdoc/tsdoc, TS-program scope, UTF-16 columns) + two Node tiers.
- **Files:** `crates/commenter-cat-engine/src/provider/native/{eslint.rs,node_runtime.rs,mod.rs}`
- **Depends on:** 6.4 (parallel-safe with 6.5 — different subtree)
- **Validation:** eslint JSON fixture maps with UTF-16→byte; `reproducibility_level` correct.

### 6.7 — implement-provider-management
- **Action:** Pinning + fetch/cache + config-fingerprint + `commenter-cat doctor`; pinned default, `--system-tools` escape hatch.
- **Files:** `crates/commenter-cat-engine/src/provider/management/{mod.rs,pinning.rs,config_fingerprint.rs,doctor.rs}`
- **Depends on:** 6.5, 6.6
- **Validation:** config_hash detects settings change; doctor flags version skew + config-differs under pinned.

## Phase Validation
`cargo test -p commenter-cat-engine provider::` passes across contract, run_state, manifest, builtins, native::eslint, and management; the provider layer can run the four built-in tools (three via dogfooded manifests, eslint native) over a fixture tree in JSON-only mode, normalize their output to canonical `Finding`s, resolve run-state correctly when a provider is absent or crashes, and report a coherent `commenter-cat doctor` version+config comparison.
