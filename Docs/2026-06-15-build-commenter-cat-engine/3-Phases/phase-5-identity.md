---
plan_slug: 2026-06-15-build-commenter-cat-engine
phase: 5
name: identity
goal: "Implement cross-scan comment identity: the cosmetic_fingerprint, the composite (bound_symbol, kind, cosmetic_fingerprint) identity, and the four-tier matching with per-use-case precision."
depends_on_phases: [2, 4]
parallel_safe_with_phases: [6]
tasks:
  - id: "5.1"
    name: implement-cosmetic-fingerprint
    action: "Create the cosmetic_fingerprint in commenter-cat-core: hash(normalize(text)) where normalize strips the delimiter, KEEPS the marker (TODO→FIXME is a real change), collapses whitespace, lowercases, and strips trailing punctuation — lexical only, no stemming, no stopword removal."
    files: [crates/commenter-cat-core/src/identity/mod.rs, crates/commenter-cat-core/src/identity/fingerprint.rs]
    depends_on: []
    parallel_safe: true
    validation: "cargo test -p commenter-cat-core identity::fingerprint passes: whitespace/case/trailing-punct edits preserve the fingerprint, but a TODO→FIXME marker change alters it"
    notes: "Idea §4 Comment identity. Lexical-only is deliberate. Property-tested later for stability (§11)."
  - id: "5.2"
    name: implement-composite-identity
    action: "Create the composite identity type identity = (bound_symbol, kind, cosmetic_fingerprint) with an ordinal tie-breaker for collisions; integrate it into the index.db identity table so identity survives line shifts."
    files: [crates/commenter-cat-core/src/identity/mod.rs, crates/commenter-cat-engine/src/storage/identity_store.rs]
    depends_on: ["5.1"]
    parallel_safe: false
    validation: "cargo test -p commenter-cat-engine storage::identity_store passes: two comments with the same (bound_symbol, kind, fingerprint) disambiguate by ordinal; identity is stable across a line shift"
    notes: "Idea §4: bound_symbol anchors location so identity survives line shifts. Writes to the identity table created in Phase 4 (4.2). Spans commenter-cat-core type + commenter-cat-engine store."
  - id: "5.3"
    name: implement-tiered-matching
    action: "Create the tiered matcher per Idea §4: Tier 1 exact (content_hash) for cache hits, Tier 2 cosmetic (same bound_symbol+fingerprint) for suppression/baseline, Tier 3 relocated (same fingerprint, different bound_symbol) for refactor-following, Tier 4 reworded (same bound_symbol+kind, similarity ≥ τ≈0.8, no rival) for issue/blame continuity ONLY — never suppression."
    files: [crates/commenter-cat-engine/src/identity/matcher.rs, crates/commenter-cat-engine/src/identity/mod.rs]
    depends_on: ["5.2"]
    parallel_safe: false
    validation: "cargo test -p commenter-cat-engine identity::matcher passes: each tier resolves on its fixture, and Tier 4 (fuzzy) is rejected for a suppression query while accepted for an issue-continuity query"
    notes: "Idea §4 Matching table: precision per use-case. Tier 4 fuzzy NEVER suppresses (a false match would hide a real finding). τ≈0.8 is a named const, not a magic number (general.md ORG_MAGIC_NUMBER). Tier 4 uses vector similarity from Phase 4."
---

# Phase 5: Identity

## Goal
Implement cross-scan comment identity so a one-word edit never orphans a suppression or re-files an issue. The composite `(bound_symbol, kind, cosmetic_fingerprint)` anchors identity through line shifts, and the four-tier matcher applies the right precision per use case — exact for cache, cosmetic for suppression/baseline (a false match there would hide a real finding), relocated for refactors, and fuzzy *only* for issue/blame continuity. Depends on Phase 2 (`bound_symbol`, `kind`) and Phase 4 (the identity table + vector similarity for Tier 4). Runs in parallel with Phase 6 (providers).

## Tasks

### 5.1 — implement-cosmetic-fingerprint
- **Action:** `cosmetic_fingerprint = hash(normalize(text))` — strip delimiter, keep marker, collapse whitespace, lowercase, strip trailing punctuation; lexical only.
- **Files:** `crates/commenter-cat-core/src/identity/{mod.rs,fingerprint.rs}`
- **Depends on:** none (within phase)
- **Validation:** cosmetic edits preserve fingerprint; marker change alters it.

### 5.2 — implement-composite-identity
- **Action:** Composite `(bound_symbol, kind, cosmetic_fingerprint)` + ordinal tie-break; integrate into the identity store.
- **Files:** `crates/commenter-cat-core/src/identity/mod.rs`, `crates/commenter-cat-engine/src/storage/identity_store.rs`
- **Depends on:** 5.1
- **Validation:** ordinal disambiguation; identity stable across line shift.

### 5.3 — implement-tiered-matching
- **Action:** Four-tier matcher (exact / cosmetic / relocated / reworded) with per-use-case precision.
- **Files:** `crates/commenter-cat-engine/src/identity/{matcher.rs,mod.rs}`
- **Depends on:** 5.2
- **Validation:** each tier resolves; Tier 4 rejected for suppression, accepted for issue continuity.
- τ≈0.8 is a named constant; Tier 4 uses Phase 4 vector similarity.

## Phase Validation
`cargo test -p commenter-cat-core identity:: && cargo test -p commenter-cat-engine identity:: storage::identity_store` pass; the same logical comment is matched correctly across a simulated re-scan involving a whitespace edit (Tier 2 holds), a relocation (Tier 3), and a rewording (Tier 4 for issue continuity only), with the fuzzy tier provably excluded from suppression.
