# CLAUDE.md

Commenter-Cat: a deterministic, multi-language comment-intelligence engine in Rust.
Walk → extract (tree-sitter) → map comment→code → delegate rule-checking to external linters
→ normalize into one `Finding` model → index in SQLite → expose as CLI verbs + an MCP surface.
The MCP surface driven live by an agent is the product; everything else is substrate.

## Workspace

Cargo workspace, three crates, strict one-way dependency `commenter-cat-cli → commenter-cat-engine → commenter-cat-core`:

- `commenter-cat-core` — **pure domain**. Errors, versioned contracts, layered config, `Finding`,
  `Comment`, primitives. Imports **no** infrastructure (no SQLite, subprocess, git, tree-sitter).
- `commenter-cat-engine` — **sole owner of infrastructure**. Walk, extract, map, git, storage, providers,
  MCP, render, ops. Translates every infra failure into `commenter_cat_core::CommenterCatError` at the adapter seam.
- `commenter-cat-cli` — the thin `commenter-cat` binary (`crates/commenter-cat-cli`, `[[bin]] name = "commenter-cat"`). clap surface only;
  verbs map 1:1 onto the MCP tools.

Version/edition/MSRV are workspace-inherited — bump in the root `Cargo.toml`
`[workspace.package]`, never per-crate. MSRV 1.96, edition 2021.

## Build / test / lint

- `cargo build` · `cargo build --release -p commenter-cat-cli`
- `cargo test --workspace` — the full suite. One real-ONNX embedding test is `#[ignore]`d to
  keep the suite offline/fast; run it with `cargo test -p commenter-cat-engine -- --ignored`.
- `cargo clippy --workspace --all-targets -- -D warnings` — **the gate**. CI denies warnings.
- `cargo fmt --all --check`
- `cargo bench -p commenter-cat-engine --bench native_pass`

## Conventions & gotchas

- **`-D warnings` is enforced in CI.** Clippy guards are workspace lints: `unwrap_used`,
  `expect_used`, `panic`, `todo`, `unimplemented`, `dbg_macro`, `print_stdout`, `print_stderr`
  are all `warn` → error in CI. Don't add `.unwrap()`/`println!` to non-test code.
- **`print_stdout`/`print_stderr` are denied workspace-wide.** Only `commenter-cat-cli` prints: its verb
  handlers own stdout via explicit writers, and `main.rs` has a local `#![allow(clippy::print_stderr)]`.
  Never emit to stdout/stderr from `commenter-cat-core` or `commenter-cat-engine`.
- **`unsafe_code = "deny"`** (not forbid) so the single FFI seam — registering the statically
  linked `sqlite-vec` auto-extension in `storage/connection.rs` — can opt in with a justified
  local `#[allow(unsafe_code)]`. All other code stays safe. Don't add new `unsafe`.
- **`missing_docs = "warn"`** — public items need a doc comment.
- **The write path is parse-invariant — never write source directly.** Comment edits must go
  through `ops/apply` (re-parse + assert the code-node tree is byte-identical, abort on any
  delta); `ops/strip` is its bulk sweep and reuses it per comment rather than rewriting text.
  Behavior-bearing kinds (directive / shebang / encoding-decl) need `allow_significant`.
  A Python docstring *deletion* leaves no string to exclude, so the token stream alone
  decides — which is why dropping a symbol's **only** docstring still aborts (the emptied
  suite loses its indent/dedent tokens) rather than writing an `IndentationError` to disk.
- **Errors:** `CommenterCatError` (thiserror, `#[non_exhaustive]`) with per-subsystem variants; chain
  causes with `.caused_by(e)`; translate infra errors at the `commenter-cat-engine` adapter boundary.

## Providers

- External tools (ruff, eslint, shellcheck, gitleaks) are **fetched/cached at runtime, never
  bundled or linked**. A missing tool → `SKIPPED` (graceful), never a crash or false zero.
- Built-in providers **are manifests** (`crates/commenter-cat-engine/assets/providers/*.manifest.toml`,
  `include_str!`-embedded), loaded exactly like user adapters. For most provider changes, edit
  the TOML — not Rust. eslint is the one Tier-2 native exception (its Node stack).
- **gitleaks is comment-scoped** (`comment_scoped = true` capability): a secret surfaces only
  when it sits *inside a comment* (a key in a `# TODO`, a token in commented-out code). Hits
  out in code or build artifacts are dropped at fusion (never surfaced as unattached), and the
  run-state is recomputed from survivors — so an all-code run reads `EMPTY`, not a misleading
  `SUCCESS`. The veto is location-only (`within_comment_span`, the attach-by-location predicate),
  so a kept finding always attaches. Any manifest can opt in; `PARTIAL`/`SKIPPED` pass through.
- **Exit code is not the run signal** — linters exit nonzero merely on findings. Parsed JSON =
  ran; malformed/crash = `PARTIAL` (findings unavailable ≠ zero). See `provider/run_state.rs`.

## Native silent-rot detectors (`rot/`)

- **These are *native*, not providers.** No provider catches *silent rot* — a comment that
  reads like good documentation while making a factual claim about its bound code that is no
  longer true. It needs Commenter-Cat's substrate: comment→code binding (`bound_symbol` /
  `bound_node_range`), the git `BlameIndex`, and the on-device embedder. Five detectors live in
  `crates/commenter-cat-engine/src/rot/`, each a **pure function over an enriched `Comment`**
  emitting `origin = Native`, `fix = AgentOnly` findings — a token-free shortlist the agent
  judges, never a verdict.
- **The detectors + their rule ids:** reference-liveness (`rot_ref`, `reference.rs`),
  docstring↔signature contract (`rot_signature`, `signature/`), path/identifier existence
  (`rot_path`, `path_ref.rs`), git-drift (`rot_drift`, `drift.rs`, the relocated blame-skew),
  semantic contradiction (`rot_semantic`, `semantic.rs`).
- **A comment-intent classifier (`intent.rs`) is the shared gate.** It buckets a comment
  (directive / explanatory-note / log-level-reference / doc-contract) so detectors fire only on
  checkable claims and the noisy `NOTE`/`WARNING` log-level mentions stay quiet. `DocContract`
  outranks a leading marker (a docstring with a mid-body `NOTE:` is still a contract); reference
  and path checks run on every intent except `LogLevelReference`; signature and semantic run only
  on a `DocContract`.
- **`rot_pass` (`rot/mod.rs`) is the convergence point**, wired into `ops/check.rs` fusion. It
  builds the repo-wide symbol index once, then runs the detectors in a **fixed order** — reference,
  path, git-drift, signature, semantic — feeding the semantic detector `structural_clean` (no
  structural detector fired) so it never piles onto an already-flagged comment. Order and sorted
  output are load-bearing for determinism (Idea §11).
- **`[rot]` config (core `config/model.rs`)** toggles and tunes each detector. The four structural
  detectors **default on** (deterministic, low-false-positive); **semantic defaults off** until a
  dogfood pass proves it low-noise, and even on it is gated behind the structural pass and an
  integer alignment-score threshold (never a float compare). `check` uses the offline
  `DeterministicEmbedder` so it stays offline and reproducible; the real ONNX assertion is
  `#[ignore]`d like the existing embedding test.
- **Graceful degradation, generalized:** a detector that cannot run is a no-op, never a false
  finding or a crash — no git repo → git-drift skips; embeddings unavailable → semantic skips;
  unbound comment → reference/signature/semantic skip. Resolution is deliberately *generous*
  (a dangling reference is only flagged when strongly in-repo-shaped; a path resolves by basename
  or suffix) so a live symbol is never mis-flagged.

## Storage

- Two-layer per-project cache at `.commenter-cat/` (`CACHE_DIR_NAME`), **gitignored**:
  content-addressed `inputs.db` (survives schema bumps) + derived `index.db` (cheap re-derive).
- **Cache keys on content hash, not path+mtime** ("mtime is a liar"). Never commit `.commenter-cat/`.
- `commenter-cat.baseline.toml` (`BASELINE_FILENAME`) and `commenter-cat.toml`
  (`CONFIG_FILENAME`) are **committed**, beside the config, outside the cache.

## CLI status (reality vs. plan)

Every verb is wired end to end: `check`, `candidates`, `doctor`, `query`, `context`,
`apply-edit`, `remove`, `strip`, `baseline`, `suppressions export`, `issues sync`, `mcp`,
`install-hooks`. `commenter-cat check` now applies the unified suppression pass (inline `commenter-cat:*`
directives + committed baseline): suppressed findings stay in the index but are hidden
from the default view and never gate CI; `--show-suppressed` is the audit view. Three
source/network verbs carry deliberate guardrails:

- `commenter-cat strip` is the **scan-and-clean sweep** (`ops/strip.rs`): one walk, then per
  file a single splice proven code-invariant by the *same* leaf-token comparison the
  applier uses — verification is per **file**, not per comment, because per-comment
  re-parsing is quadratic exactly where a sweep lands hardest (600 comments in one file
  cost ~1200 parses, ≈2s → ~10ms). A batch that does not verify is only unsafe
  *somewhere*, so it re-runs through the single-comment applier to keep what is safe and
  name what is not. Source-mutating and repo-wide, so it **defaults to a dry-run plan**
  and rewrites only under `--apply`. Protected by kind and *counted, never silently dropped*: behavior-bearing
  comments (directive / shebang / encoding-decl) and license headers survive unless
  surrendered by `--allow-significant` / `--strip-license`; `--keep <kind>` adds
  protection. A removal leaves a blank line behind, so `strip` tidies the lines it
  touched and **re-verifies the tidy against the same leaf-token stream**, discarding it
  wholesale if it differs (`--no-tidy` opts out). Per-file degradation: an unreadable
  file, a grammar failure, or a comment the applier refuses is a recorded skip — only a
  mid-pass *write* failure is fatal. Stripping leaves the index describing comments that
  are gone; the report says so and `check` re-derives it.
- `commenter-cat suppressions export` mutates source — it materializes Commenter-Cat's suppression set into each
  tool's native directives (`# noqa`, `eslint-disable-next-line`, `# shellcheck disable`,
  `# gitleaks:allow`), **merged per line** (`# noqa: D400, D415`) and written through the
  parse-invariant applier (a code-altering insert aborts). Idempotent on the directive
  marker; Commenter-Cat-native findings have no native directive and are skipped.
- `commenter-cat issues sync` is bidirectional and network + `gh`, outward-facing — so it **defaults
  to a dry-run plan** and only mutates under `--apply`. Idempotency is a **committed**
  ledger (`commenter-cat.issues.toml`, beside the baseline, `LEDGER_FILENAME`) keyed on
  Tier-4 identity, so a marker filed by anyone is never re-filed. **Forward:** file
  marker comments not yet tracked. **Reverse (`issues::reconcile_resolved`):** a resolved
  (closed) issue → remove its marker comment through the parse-invariant applier, batched
  per file high→low; the resolved token is then dropped from the ledger. The dry-run
  resolution preview is a read-only tracker query that degrades gracefully (an unreachable
  issue is treated as not-closed — a comment is removed only on a *positive* closed
  confirmation).

## Docs

- `Docs/Idea.md` — the authoritative design (sections referenced throughout the code as "Idea §N").
- `Docs/2026-06-15-build-commenter-cat-engine/` — the execution plan; `STATUS.md` is its live
  per-task state. That directory's `CHANGELOG.md` is a **re-plan audit**, not the project
  changelog — the project changelog is the root `CHANGELOG.md`.
