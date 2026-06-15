---
plan_slug: 2026-06-15-build-commenter-cat-engine
section: changelog
schema_version: 1
entries:
  - id: dogfood-autotravian-provider-fixes
    date: 2026-06-15
    kind: bugfix
    summary: >-
      Dogfooding cf check on ~/Workspace/AutoTravian (1.8 GB, 4 langs, non-git)
      surfaced four bugs that broke the provider→finding pipeline end to end;
      all fixed, +5 regression tests (328→333).
  - id: wire-baseline-and-fix-install-hooks-help
    date: 2026-06-15
    kind: enhancement
    summary: >-
      Post-dogfood checkpoint: corrected the `cf install-hooks` clap help (it
      installs cache-warmer post-* hooks, not pre-commit/pre-push) and wired
      `cf baseline accept|prune` end to end. `suppressions`/`issues` now return
      explicit "wire deliberately" errors rather than silent stubs. +1
      regression test (333→334).
---

# Changelog

Append-only audit of re-plans and edits. Each entry records what changed and why.

## Entries

### 2026-06-15 — Dogfood on AutoTravian: provider pipeline fixes

Built `cf` (release) and ran `cf check` against `~/Workspace/AutoTravian/` — a
1.8 GB, multi-language (Python/TS/JS/Shell), **non-git** corpus with ~960 MB of
gitignored-but-present vendored deps. The native pass was flawless (15,735
comments extracted/mapped, markers + rot candidates correct), but **every
external-provider finding was silently missing**. Root-caused and fixed four
bugs; each got a regression test.

1. **Provider findings never attached (path mismatch).** `ManifestProvider`
   emitted `finding.file` as the **absolute** path the tool echoes — ruff even
   canonicalizes it (`/tmp`→`/private/tmp` on macOS) — while comments key off the
   repo-relative `path`. `attaches_to` compared the two directly, so *no*
   ruff/shellcheck/gitleaks finding ever attached; all fell into `unattached`.
   Fix: a canonicalization-aware `relativize()` in `manifest/mod.rs::normalize`.
2. **gitleaks produced nothing.** The manifest ran
   `gitleaks detect --report-path /dev/stdout {files}`; gitleaks 8.x rejects
   `/dev/stdout` ("report path is not writable") *and* `detect` ignores
   positional files (scans `--source .`). Result: empty stdout → silent
   `PARTIAL`. Fix: `gitleaks dir --report-format json --report-path - --no-banner
   {root}` + a new `{root}` manifest token for project-scoped single-path tools.
3. **Zero-width findings missed the comment's first byte.** Providers report a
   line+column that converts to a single byte; `Range::overlaps` (strict
   intersection) misses a point sitting exactly on a comment's first byte — so
   ruff `ERA001` (commented-out code, points at the `#`) never attached. Fix:
   `Range::contains_byte` + point-containment in `attaches_to`.
4. **Silent degradation.** The CLI discarded `run_states` and `unattached`, so a
   `PARTIAL`/`SKIPPED` provider (and symbol-level doc findings) vanished — exactly
   the "degraded guarantee must be visible" invariant (Idea §5). Fix:
   `report_diagnostics()` writes provider trouble + unattached counts to stderr.

Plus **finding scope-filtering** in `ops::check::fuse` so a project-scoped tool
scanning `{root}` cannot widen `cf_scope` — out-of-scope (node_modules/.venv)
secrets are dropped (Idea §5 "a provider never sees a file CF excluded").

**Post-fix result on AutoTravian:** ruff/gitleaks attach correctly (verified on a
seeded fixture: ERA001 + 2 gitleaks secrets fused onto one comment); gitleaks
found 9 real in-scope secrets (was 0) with ~196 vendored-dep/.env/.log hits
correctly filtered; `shellcheck skipped` + unattached secret counts now visible.

**Open observations (not bugs — recorded for planning):**
- *gitleaks perf:* project-scoped gitleaks scans the whole tree (~233 s on this
  repo, dominated by the 960 MB of vendored deps it traverses then we filter).
  gitleaks has no path-exclude CLI flag; the designed tree-hash provider cache
  (§6/§7) is the intended mitigation but is not wired into the `cf check` path.
- *secret scope vs §3:* the scope-filter currently keys off the comment-language
  walk, so gitleaks hits in `.env`/config files are dropped too. Idea §3 wants
  `.env.*` covered — broadening the scope set to "non-ignored" (not just
  "analyzed-language") files is the follow-up.
- *native pass is sequential:* `ops::check::native_pass` is a plain loop; rayon
  (Idea §6/§10 stack) is not actually a dependency. A perf gap, not a defect.
- *`--stats` is a no-op:* the global flag parses but emits nothing.

### 2026-06-15 — Follow-up: wire `cf baseline`, fix `install-hooks` help

A post-dogfood checkpoint flagged two items; both are closed.

1. **`cf install-hooks` help string was wrong.** `cli/mod.rs` described the verb
   as *"Install the git hooks (pre-commit / pre-push)"*, but the implementation
   installs the cache-warmer **post-commit / -checkout / -merge / -rewrite** hooks
   via `core.hooksPath` (task 9.2; Idea §7) — non-fatal, changed-files-only, never
   hand-editing `.git/hooks`. Corrected the clap doc to describe the real behavior.
2. **Wired `cf baseline accept|prune`.** The engine `ops::baseline` functions were
   complete and tested, but the verb returned a "not wired" placeholder. `run_baseline`
   now runs `check`, derives each finding's **Tier-2** identity (`bound_symbol`,
   `cosmetic_fingerprint`, `provider_rule_id`) via `current_identities`, and `accept`
   snapshots / `prune` drops-stale into the committed `comment-finder.baseline.toml`
   (the diff anchor the CI path already consumes via `ci/diff.rs`; Idea §5). Verified
   end to end on a fixture: `accept` → 4 identities written in canonical (sorted,
   deduped) order; `prune` against the same findings → 0 stale. +1 regression test
   (`test_baseline_accept_snapshots_and_prune_keeps_live`, 333→334).

`cf suppressions export` and `cf issues sync` remain **deliberately unwired** — not
silent stubs but explicit, explained errors. `suppressions export` mutates *source*
files and depends on a suppression pass `cf check` does not yet apply; `issues sync`
is network- + `gh`-backed and outward-facing (files issues against the repo). Both
are sound engine modules — enabling them is a deliberate act, not a default.

**Open observation (not a bug):** the repo-root `CLAUDE.md` "CLI status" section
still lists `cf baseline` among the not-wired verbs — stale after this change. That
file is owner-authored (created outside `cf`'s only write path, which is comment
edits); left untouched and flagged rather than edited.

Gate after the follow-up: **334 tests** (was 333), clippy `-D warnings` clean, fmt clean.
