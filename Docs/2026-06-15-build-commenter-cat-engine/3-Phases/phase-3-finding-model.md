---
plan_slug: 2026-06-15-build-commenter-cat-engine
phase: 3
name: finding-model
goal: "Define the canonical normalized Finding model, the category-anchored 4-level severity resolution, the coordinate-conversion layer, and the same-category dedup rule — the schema every provider and native check reconciles to."
depends_on_phases: [1]
parallel_safe_with_phases: [2]
tasks:
  - id: "3.1"
    name: define-finding-struct
    action: "Create the canonical Finding struct in commenter-cat-core per Idea §5: fields origin (enum ruff|eslint|shellcheck|gitleaks|native), provider_rule_id (lossless origin-qualified), canonical_rule_id, category (enum: doc_missing, doc_drift, doc_style, commented_code, secret, todo_format, marker_stale, comment_style, shebang, directive, rot_candidate), severity, severity_native, message, target (comment_id|bound_symbol), range, fix (provider_autofix|agent_only|none), url; plus the Category enum and a FindingTarget enum."
    files: [crates/commenter-cat-core/src/finding/mod.rs, crates/commenter-cat-core/src/finding/category.rs, crates/commenter-cat-core/src/lib.rs]
    depends_on: []
    parallel_safe: true
    validation: "cargo test -p commenter-cat-core finding:: passes; serde round-trip of a Finding is byte-stable"
    notes: "Idea §5 Finding block is authoritative. provider_rule_id is NEVER discarded (rule identity is never lossy). String literal unions → Rust enums (general.md type rigor)."
  - id: "3.2"
    name: implement-severity-resolution
    action: "Create the severity resolver implementing Idea §5 order (first hit wins): (1) config override by rule or category, (2) category canonical default table (secret=critical; doc_drift/directive-malformed=error; commented_code/doc_missing/marker_stale/shebang=warning; doc_style/comment_style/todo_format/rot_candidate=info), (3) per-tool translation table (eslint 2/1, shellcheck error/warning/info/style, gitleaks=critical) recorded as severity_native."
    files: [crates/commenter-cat-core/src/finding/severity.rs, crates/commenter-cat-core/src/finding/mod.rs]
    depends_on: ["3.1"]
    parallel_safe: false
    validation: "cargo test -p commenter-cat-core finding::severity passes: config override beats category, category beats per-tool, and doc_drift resolves to error from both ruff and eslint origins"
    notes: "Idea §5 severity tables. Category-anchored = cross-language consistency. fail_on default = error (§5/§12). No magic numbers — table is named consts."
  - id: "3.3"
    name: implement-coordinate-conversion
    action: "Create the coordinate-conversion module mapping provider coordinate systems to the engine's byte-offset system: handle 1-based vs 0-based, and convert eslint UTF-16 columns to byte offsets (tree-sitter/ruff are byte/char); expose a declared-convention enum (1-based-utf8, etc.) consumed by manifest capabilities later."
    files: [crates/commenter-cat-core/src/finding/coordinates.rs, crates/commenter-cat-core/src/finding/mod.rs]
    depends_on: ["3.1"]
    parallel_safe: false
    validation: "cargo test -p commenter-cat-core finding::coordinates passes, including a UTF-16 column with a multi-byte char before it and a 0-based↔1-based case"
    notes: "Idea §5 Coordinate hygiene + §5 invocation Locations. UTF-16↔byte is property-tested later (§11). Same module dir as 3.2 but different file; sequenced after 3.1 which defines range."
  - id: "3.4"
    name: implement-finding-dedup
    action: "Create the dedup rule: two findings of the same category overlapping one comment are merged — highest severity kept, origins unioned; produce a stable canonical sort (file, line, col, provider_rule_id) for deterministic baselines/diffs."
    files: [crates/commenter-cat-core/src/finding/dedup.rs, crates/commenter-cat-core/src/finding/mod.rs]
    depends_on: ["3.2", "3.3"]
    parallel_safe: false
    validation: "cargo test -p commenter-cat-core finding::dedup passes: overlapping same-category findings collapse to one with unioned origins and max severity, and output order is canonical"
    notes: "Idea §5 Coordinate + dedup hygiene + invocation Order. Canonical ordering is what makes two CI runs byte-identical (§7)."
---

# Phase 3: Finding Model

## Goal
Define the one schema every provider adapter and every native check reconciles to: the canonical `Finding`, the category-anchored 4-level severity resolution, the coordinate-conversion layer (the eslint UTF-16 trap), and the same-category dedup with canonical ordering. This phase lives in `commenter-cat-core` and depends only on Phase 1, so it runs in parallel with Phase 2 (native substrate). It is a hard prerequisite for both Phase 4 storage (which persists findings) and Phase 6 providers (which produce them).

## Tasks

### 3.1 — define-finding-struct
- **Action:** Canonical `Finding` struct + `Category`/`FindingTarget` enums per Idea §5.
- **Files:** `crates/commenter-cat-core/src/finding/{mod.rs,category.rs}`, `crates/commenter-cat-core/src/lib.rs`
- **Depends on:** none (within phase; phase depends on P1)
- **Validation:** `cargo test -p commenter-cat-core finding::`; serde round-trip byte-stable.
- `provider_rule_id` is lossless and never discarded.

### 3.2 — implement-severity-resolution
- **Action:** Severity resolver with the three-tier order (config → category default → per-tool table).
- **Files:** `crates/commenter-cat-core/src/finding/severity.rs`, `crates/commenter-cat-core/src/finding/mod.rs`
- **Depends on:** 3.1
- **Validation:** override > category > per-tool; `doc_drift` → error from both ruff and eslint.

### 3.3 — implement-coordinate-conversion
- **Action:** Convert provider coordinates (1/0-based, eslint UTF-16) to engine byte offsets; declared-convention enum.
- **Files:** `crates/commenter-cat-core/src/finding/coordinates.rs`, `crates/commenter-cat-core/src/finding/mod.rs`
- **Depends on:** 3.1
- **Validation:** UTF-16 column with preceding multi-byte char + 0/1-based case.

### 3.4 — implement-finding-dedup
- **Action:** Merge overlapping same-category findings (max severity, unioned origins); canonical sort.
- **Files:** `crates/commenter-cat-core/src/finding/dedup.rs`, `crates/commenter-cat-core/src/finding/mod.rs`
- **Depends on:** 3.2, 3.3
- **Validation:** overlap collapses correctly; output order canonical.
- Canonical ordering underpins byte-identical CI reports (§7).

## Phase Validation
`cargo test -p commenter-cat-core finding::` passes end-to-end: a mixed list of provider-shaped and native-shaped findings can be normalized into canonical `Finding`s, severity-resolved, coordinate-converted, deduped, and emitted in canonical order deterministically across two runs.
