# CLAUDE.md

Commenter-Cat (`cf`): a deterministic, multi-language comment-intelligence engine in Rust.
Walk → extract (tree-sitter) → map comment→code → delegate rule-checking to external linters
→ normalize into one `Finding` model → index in SQLite → expose as CLI verbs + an MCP surface.
The MCP surface driven live by an agent is the product; everything else is substrate.

## Workspace

Cargo workspace, three crates, strict one-way dependency `cf-cli → cf-engine → cf-core`:

- `cf-core` — **pure domain**. Errors, versioned contracts, layered config, `Finding`,
  `Comment`, primitives. Imports **no** infrastructure (no SQLite, subprocess, git, tree-sitter).
- `cf-engine` — **sole owner of infrastructure**. Walk, extract, map, git, storage, providers,
  MCP, render, ops. Translates every infra failure into `cf_core::CfError` at the adapter seam.
- `cf-cli` — the thin `cf` binary (`crates/cf-cli`, `[[bin]] name = "cf"`). clap surface only;
  verbs map 1:1 onto the MCP tools.

Version/edition/MSRV are workspace-inherited — bump in the root `Cargo.toml`
`[workspace.package]`, never per-crate. MSRV 1.96, edition 2021.

## Build / test / lint

- `cargo build` · `cargo build --release -p cf-cli`
- `cargo test --workspace` — the full suite. One real-ONNX embedding test is `#[ignore]`d to
  keep the suite offline/fast; run it with `cargo test -p cf-engine -- --ignored`.
- `cargo clippy --workspace --all-targets -- -D warnings` — **the gate**. CI denies warnings.
- `cargo fmt --all --check`
- `cargo bench -p cf-engine --bench native_pass`

## Conventions & gotchas

- **`-D warnings` is enforced in CI.** Clippy guards are workspace lints: `unwrap_used`,
  `expect_used`, `panic`, `todo`, `unimplemented`, `dbg_macro`, `print_stdout`, `print_stderr`
  are all `warn` → error in CI. Don't add `.unwrap()`/`println!` to non-test code.
- **`print_stdout`/`print_stderr` are denied workspace-wide.** Only `cf-cli` prints: its verb
  handlers own stdout via explicit writers, and `main.rs` has a local `#![allow(clippy::print_stderr)]`.
  Never emit to stdout/stderr from `cf-core` or `cf-engine`.
- **`unsafe_code = "deny"`** (not forbid) so the single FFI seam — registering the statically
  linked `sqlite-vec` auto-extension in `storage/connection.rs` — can opt in with a justified
  local `#[allow(unsafe_code)]`. All other code stays safe. Don't add new `unsafe`.
- **`missing_docs = "warn"`** — public items need a doc comment.
- **The write path is parse-invariant — never write source directly.** Comment edits must go
  through `ops/apply` (re-parse + assert the code-node tree is byte-identical, abort on any
  delta). Behavior-bearing kinds (directive / shebang / encoding-decl) need `allow_significant`.
- **Errors:** `CfError` (thiserror, `#[non_exhaustive]`) with per-subsystem variants; chain
  causes with `.caused_by(e)`; translate infra errors at the `cf-engine` adapter boundary.

## Providers

- External tools (ruff, eslint, shellcheck, gitleaks) are **fetched/cached at runtime, never
  bundled or linked**. A missing tool → `SKIPPED` (graceful), never a crash or false zero.
- Built-in providers **are manifests** (`crates/cf-engine/assets/providers/*.manifest.toml`,
  `include_str!`-embedded), loaded exactly like user adapters. For most provider changes, edit
  the TOML — not Rust. eslint is the one Tier-2 native exception (its Node stack).
- **Exit code is not the run signal** — linters exit nonzero merely on findings. Parsed JSON =
  ran; malformed/crash = `PARTIAL` (findings unavailable ≠ zero). See `provider/run_state.rs`.

## Storage

- Two-layer per-project cache at `.commenter-cat/` (`CACHE_DIR_NAME`), **gitignored**:
  content-addressed `inputs.db` (survives schema bumps) + derived `index.db` (cheap re-derive).
- **Cache keys on content hash, not path+mtime** ("mtime is a liar"). Never commit `.commenter-cat/`.
- `commenter-cat.baseline.toml` (`BASELINE_FILENAME`) and `commenter-cat.toml`
  (`CONFIG_FILENAME`) are **committed**, beside the config, outside the cache.

## CLI status (reality vs. plan)

Every verb is wired end to end: `check`, `candidates`, `doctor`, `query`, `context`,
`apply-edit`, `remove`, `baseline`, `suppressions export`, `issues sync`, `mcp`,
`install-hooks`. `cf check` now applies the unified suppression pass (inline `cf:*`
directives + committed baseline): suppressed findings stay in the index but are hidden
from the default view and never gate CI; `--show-suppressed` is the audit view. Two
source/network verbs carry deliberate guardrails:

- `cf suppressions export` mutates source — it materializes CF's suppression set into each
  tool's native directives (`# noqa`, `eslint-disable-next-line`, `# shellcheck disable`,
  `# gitleaks:allow`), **merged per line** (`# noqa: D400, D415`) and written through the
  parse-invariant applier (a code-altering insert aborts). Idempotent on the directive
  marker; CF-native findings have no native directive and are skipped.
- `cf issues sync` is bidirectional and network + `gh`, outward-facing — so it **defaults
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
