---
plan_slug: 2026-06-15-build-commenter-cat-engine
phase: 9
name: adjacent-integrations
goal: "Build the adjacent (non-loop-critical) integrations: comment-to-issue with pluggable trackers, the git-hook cache-warmer installer, and CI integration with the two-artifact cache and degraded-diff verdict."
depends_on_phases: [8]
parallel_safe_with_phases: [10]
tasks:
  - id: "9.1"
    name: implement-comment-to-issue
    action: "Create comment-to-issue behind a small issue-backend interface (GitHub octocrab, Jira, GitLab pluggable): mechanical creation idempotent on issue_url stored against the comment's Tier-4 identity (no double-file on rewordings), auth via host credentials (gh/GITHUB_TOKEN or env/secrets-manager, never config/index), and opt-in bidirectional `commenter-cat issues sync` routing marker-resolution through the parse-invariant applier."
    files: [crates/commenter-cat-engine/src/issues/mod.rs, crates/commenter-cat-engine/src/issues/backend.rs, crates/commenter-cat-engine/src/issues/github.rs]
    depends_on: []
    parallel_safe: true
    validation: "cargo test -p commenter-cat-engine issues:: passes with a mocked tracker seam: creation is idempotent on Tier-4 identity, a reworded comment does not double-file, and sync routes marker-resolution through the applier"
    notes: "Idea §9: edge of the loop, not its center — never on the find→update→re-check critical path. Tier-4 identity (Phase 5). Mock only the network seam (§11). Auth never stored. Jira/GitLab backends are the same interface; GitHub is the worked impl here."
  - id: "9.2"
    name: implement-git-hooks
    action: "Create `commenter-cat install-hooks`: distribute hooks via core.hooksPath (or lefthook/pre-commit manager), covering post-commit/post-checkout/post-merge/post-rewrite; the hook scans only the commit's changed files (git diff-tree from 2.6) and runs providers only on those — fast, non-fatal, never blocking; never hand-edit .git/hooks/."
    files: [crates/commenter-cat-engine/src/hooks/mod.rs, crates/commenter-cat-engine/src/hooks/install.rs, crates/commenter-cat-cli/src/cli/verbs.rs]
    depends_on: []
    parallel_safe: true
    validation: "cargo test -p commenter-cat-engine hooks:: passes: install writes via core.hooksPath, the four events are covered, and the warm path scans only changed files"
    notes: "Idea §7: the hook warms the cache, does NOT own the data (source of truth = scanner-on-demand). Reuses changed-file sets (2.6). Touches commenter-cat-cli verbs.rs (8.1) for the install-hooks verb wiring → keep edit additive."
  - id: "9.3"
    name: implement-ci-integration
    action: "Create the CI integration: restore two cache artifacts with two keys — inputs.db keyed (provider_versions + config_hashes + inputs_schema_version) so a Commenter-Cat upgrade reuses provider results, index.db keyed on the full comparability key (re-derived from inputs.db on miss, not cold-scanned); diff vs the committed baseline failing on findings ≥ fail_on not baselined; a PARTIAL provider downgrades the verdict to degraded; publish SARIF + markdown summary + JSONL + updated cache artifacts."
    files: [crates/commenter-cat-engine/src/ci/mod.rs, crates/commenter-cat-engine/src/ci/cache_artifacts.rs, crates/commenter-cat-engine/src/ci/diff.rs]
    depends_on: ["9.2"]
    parallel_safe: false
    validation: "cargo test -p commenter-cat-engine ci:: passes: an inputs.db hit across a simulated Commenter-Cat-version bump skips provider re-runs, an index.db miss re-derives from inputs.db, and a PARTIAL provider yields a degraded verdict instead of a silent pass"
    notes: "Idea §7 CI integration: shared truth is CI. Two-layer cache keying (§6/§7). Comparability key from Phase 6 (6.7). Degraded-diff on PARTIAL (run-state, 6.2). Byte-identical reports follow from canonical ordering (3.4)."
---

# Phase 9: Adjacent Integrations

## Goal
Build the integrations the design positions as *adjacent* — useful but never on the live-session loop's critical path (Idea §9, §12). `comment-to-issue` files/syncs issues idempotently behind a pluggable tracker interface; `commenter-cat install-hooks` warms the cache on the events that change comments without ever owning the data; and the CI integration restores the two-layer cache so a Commenter-Cat upgrade reuses expensive provider results, diffs against the committed baseline, and downgrades to a degraded verdict on a PARTIAL provider rather than passing silently. Depends on Phase 8 (CLI/MCP verbs these features extend). Runs in parallel with Phase 10 (distribution + tests).

## Tasks

### 9.1 — implement-comment-to-issue
- **Action:** Pluggable issue-backend interface (GitHub/Jira/GitLab) with idempotent creation on Tier-4 identity, host-credential auth, opt-in bidirectional sync via the applier.
- **Files:** `crates/commenter-cat-engine/src/issues/{mod.rs,backend.rs,github.rs}`
- **Depends on:** none (within phase)
- **Validation:** idempotent on Tier-4; no double-file on reword; sync routes through applier (mocked tracker seam).

### 9.2 — implement-git-hooks
- **Action:** `commenter-cat install-hooks` via `core.hooksPath`/manager, four events, changed-files-only warm scan, non-fatal.
- **Files:** `crates/commenter-cat-engine/src/hooks/{mod.rs,install.rs}`, `crates/commenter-cat-cli/src/cli/verbs.rs` (additive verb wiring)
- **Depends on:** none (within phase)
- **Validation:** install via core.hooksPath; four events covered; warm path scans only changed files.

### 9.3 — implement-ci-integration
- **Action:** Two-artifact/two-key cache restore, baseline diff, degraded verdict on PARTIAL, publish SARIF/summary/JSONL/cache.
- **Files:** `crates/commenter-cat-engine/src/ci/{mod.rs,cache_artifacts.rs,diff.rs}`
- **Depends on:** 9.2
- **Validation:** inputs.db hit skips re-runs across a Commenter-Cat-version bump; index.db miss re-derives; PARTIAL → degraded.

## Phase Validation
`cargo test -p commenter-cat-engine issues:: hooks:: ci::` pass; a simulated CI run restores the two cache layers (reusing provider results across a Commenter-Cat-version bump), diffs unified findings against a committed baseline, reports a degraded verdict when a provider is PARTIAL, and the comment-to-issue + hook features operate without ever blocking or appearing on the find→update→re-check critical path.
