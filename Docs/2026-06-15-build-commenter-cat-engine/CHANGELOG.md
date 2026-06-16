---
plan_slug: 2026-06-15-build-commenter-cat-engine
section: changelog
schema_version: 1
entries:
  - id: dogfood-autotravian-provider-fixes
    date: 2026-06-15
    kind: bugfix
    summary: >-
      Dogfooding commenter-cat check on ~/Workspace/AutoTravian (1.8 GB, 4 langs, non-git)
      surfaced four bugs that broke the provider→finding pipeline end to end;
      all fixed, +5 regression tests (328→333).
  - id: wire-baseline-and-fix-install-hooks-help
    date: 2026-06-15
    kind: enhancement
    summary: >-
      Post-dogfood checkpoint: corrected the `commenter-cat install-hooks` clap help (it
      installs cache-warmer post-* hooks, not pre-commit/pre-push) and wired
      `commenter-cat baseline accept|prune` end to end. `suppressions`/`issues` now return
      explicit "wire deliberately" errors rather than silent stubs. +1
      regression test (333→334).
  - id: env-secret-scope-and-parallel-native-pass
    date: 2026-06-15
    kind: enhancement
    summary: >-
      Closed two dogfood open-observations: secret scanning now covers the full
      commenter_cat_scope universe (.env/config via walk_universe, Idea §3/§5), and the
      native pass runs in parallel via rayon (deterministic — identical output
      order). +1 regression test (334→335).
  - id: wire-provider-result-cache
    date: 2026-06-16
    kind: enhancement
    summary: >-
      Wired the §6 content-addressed provider-result cache into commenter-cat check (it
      existed but had no caller): a provider never re-runs on an unchanged input
      set, so gitleaks stops rescanning the whole tree (~233s) every run. Adds
      RuleProvider::version_key, project-scope tree-hash keying, --no-cache, and
      --stats cache reporting. +3 tests (335→338). Closes the dogfood
      gitleaks-perf open observation.
---

# Changelog

Append-only audit of re-plans and edits. Each entry records what changed and why.

## Entries

### 2026-06-15 — Dogfood on AutoTravian: provider pipeline fixes

Built `commenter-cat` (release) and ran `commenter-cat check` against `~/Workspace/AutoTravian/` — a
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
scanning `{root}` cannot widen `commenter_cat_scope` — out-of-scope (node_modules/.venv)
secrets are dropped (Idea §5 "a provider never sees a file Commenter-Cat excluded").

**Post-fix result on AutoTravian:** ruff/gitleaks attach correctly (verified on a
seeded fixture: ERA001 + 2 gitleaks secrets fused onto one comment); gitleaks
found 9 real in-scope secrets (was 0) with ~196 vendored-dep/.env/.log hits
correctly filtered; `shellcheck skipped` + unattached secret counts now visible.

**Open observations (not bugs — recorded for planning):**
- *gitleaks perf:* project-scoped gitleaks scans the whole tree (~233 s on this
  repo, dominated by the 960 MB of vendored deps it traverses then we filter).
  gitleaks has no path-exclude CLI flag; the designed tree-hash provider cache
  (§6/§7) is the intended mitigation but is not wired into the `commenter-cat check` path.
- *secret scope vs §3:* the scope-filter currently keys off the comment-language
  walk, so gitleaks hits in `.env`/config files are dropped too. Idea §3 wants
  `.env.*` covered — broadening the scope set to "non-ignored" (not just
  "analyzed-language") files is the follow-up.
- *native pass is sequential:* `ops::check::native_pass` is a plain loop; rayon
  (Idea §6/§10 stack) is not actually a dependency. A perf gap, not a defect.
- *`--stats` is a no-op:* the global flag parses but emits nothing.

### 2026-06-15 — Follow-up: wire `commenter-cat baseline`, fix `install-hooks` help

A post-dogfood checkpoint flagged two items; both are closed.

1. **`commenter-cat install-hooks` help string was wrong.** `cli/mod.rs` described the verb
   as *"Install the git hooks (pre-commit / pre-push)"*, but the implementation
   installs the cache-warmer **post-commit / -checkout / -merge / -rewrite** hooks
   via `core.hooksPath` (task 9.2; Idea §7) — non-fatal, changed-files-only, never
   hand-editing `.git/hooks`. Corrected the clap doc to describe the real behavior.
2. **Wired `commenter-cat baseline accept|prune`.** The engine `ops::baseline` functions were
   complete and tested, but the verb returned a "not wired" placeholder. `run_baseline`
   now runs `check`, derives each finding's **Tier-2** identity (`bound_symbol`,
   `cosmetic_fingerprint`, `provider_rule_id`) via `current_identities`, and `accept`
   snapshots / `prune` drops-stale into the committed `commenter-cat.baseline.toml`
   (the diff anchor the CI path already consumes via `ci/diff.rs`; Idea §5). Verified
   end to end on a fixture: `accept` → 4 identities written in canonical (sorted,
   deduped) order; `prune` against the same findings → 0 stale. +1 regression test
   (`test_baseline_accept_snapshots_and_prune_keeps_live`, 333→334).

`commenter-cat suppressions export` and `commenter-cat issues sync` remain **deliberately unwired** — not
silent stubs but explicit, explained errors. `suppressions export` mutates *source*
files and depends on a suppression pass `commenter-cat check` does not yet apply; `issues sync`
is network- + `gh`-backed and outward-facing (files issues against the repo). Both
are sound engine modules — enabling them is a deliberate act, not a default.

**Open observation (not a bug):** the repo-root `CLAUDE.md` "CLI status" section
still lists `commenter-cat baseline` among the not-wired verbs — stale after this change. That
file is owner-authored (created outside `commenter-cat`'s only write path, which is comment
edits); left untouched and flagged rather than edited.

Gate after the follow-up: **334 tests** (was 333), clippy `-D warnings` clean, fmt clean.

### 2026-06-15 — Follow-up: `.env`/config secret scope + parallel native pass

Closed two of the open observations recorded after the dogfood pass.

1. **Secret scope now covers the `commenter_cat_scope` universe (Idea §3/§5).** The
   provider-finding scope filter in `ops::check::fuse` keyed off the
   comment-language file set, so a gitleaks secret in `.env`/config — files Commenter-Cat
   deliberately keeps out of the comment grammar (Idea §3) — was dropped along with
   genuinely out-of-scope hits. Added `walk::walk_universe`: every non-ignored file
   under the root (honoring `.gitignore` + `extra_ignores`), including config
   dotfiles like `.env`, never descending into `.git`, with no language filter and
   no generated/minified sniff. `fuse` now validates provider findings against this
   universe, so a `.env` secret is **kept** (surfaced as unattached — it maps to no
   comment) while a gitignored/excluded hit is still dropped. `check()` now walks
   once for the comment-language set (it previously walked twice identically) plus
   once for the universe. +1 regression test (`.env` kept, gitignored dropped).
2. **Native pass parallelized with rayon.** `ops::check::native_pass` ran a
   sequential per-file loop; the per-file work (read + tree-sitter extract +
   coalesce + comment→code map + marker tag) is independent and CPU-bound. Extracted
   a `native_pass_file` helper and fanned it across the rayon pool. **Determinism is
   preserved** — `walk` returns a sorted slice and an indexed parallel `collect`
   writes results back in index order, so the fused stream is identical to the
   sequential pass (Idea §11); the golden, property, and integration suites pass
   unchanged. Added `rayon` to `commenter-cat-engine` (already present transitively).

Gate: **335 tests** (was 334), clippy `-D warnings` clean, fmt clean.

**Remaining open observations** (from the dogfood entry, still not addressed): the
gitleaks project-scoped tree-scan is slow on large trees (the §6/§7 tree-hash
provider cache is the intended mitigation, not yet wired into `commenter-cat check`); `--stats`
parses but is a no-op.

### 2026-06-16 — Wire the provider-result cache into `commenter-cat check` (Idea §6)

Closes the headline open observation from the dogfood pass — gitleaks rescanning
the whole tree (~233 s) on every run.

The content-addressed cache (`inputs.db` `provider_results`, keyed
`(content_hash, provider, version)`) was fully built and tested but **had no
caller**: `ops::check` always invoked every provider. Wired it in:

- **`RuleProvider::version_key()`** (default `None`) — for a manifest provider, the
  resolved tool binary's content hash folded with the manifest source hash, so a
  tool upgrade *or* a manifest edit invalidates cached findings (§5 comparability).
  `None` disables caching (in-process natives, absent tools, mocks).
- **`ops::provider_cache`** wraps each provider run: a project-scoped tool keys on
  the `commenter_cat_scope` universe tree-hash — gitleaks's *retained* findings depend only on
  universe content (out-of-scope hits are filtered), so that key is correct and
  sufficient — and a file-scoped tool on the comment-language file set. The raw
  output is cached; the existing scope filter still runs after. Only `SUCCESS`/
  `EMPTY` are cached; the cache is best-effort (any failure → run, never a wrong
  result).
- **Cache-dir exclusion** — Commenter-Cat's own `.commenter-cat` is now pruned from the
  universe walk (alongside `.git`); otherwise its mutating `inputs.db`/`index.db`
  would perturb the tree hash and self-invalidate the cache every run.
- **CLI** — `check()` gained `use_cache`; `--no-cache` bypasses it; `--stats` now
  reports cache hits vs runs (was a no-op on this dimension).

Verified end to end with the real providers: a second `commenter-cat check` on an unchanged
tree serves ruff + gitleaks from cache (gitleaks does not rescan), a content change
invalidates and re-runs all, and the `.env` secret is cached and re-surfaced.
Gate: **338 tests** (was 335), clippy `-D warnings` clean, fmt clean.

**Remaining open observation:** `--stats` still does not report full per-stage
timing (the §6 budget shape) — only the provider-cache dimension is wired so far.
