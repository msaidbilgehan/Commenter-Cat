# Commenter-Cat

[![CI](https://github.com/msaidbilgehan/Commenter-Cat/actions/workflows/ci.yml/badge.svg)](https://github.com/msaidbilgehan/Commenter-Cat/actions/workflows/ci.yml)
[![License: BSD-3-Clause](https://img.shields.io/badge/License-BSD--3--Clause-blue.svg)](LICENSE)

A deterministic, multi-language **comment-intelligence engine** — a Rust orchestrator that
walks a directory, extracts comments with tree-sitter, **maps each comment to the code it
annotates**, delegates per-language rule-checking to best-in-class external tools (ruff,
eslint + jsdoc/tsdoc, shellcheck, gitleaks), **normalizes** their findings into one unified
model, enriches with git, indexes everything in a per-project SQLite cache (keyword + vector
search), and exposes the whole thing as an **agent-facing surface** (MCP tools = CLI verbs).

**That agent-facing surface, driven live by a coding agent, is the product.** Everything else
is substrate that makes the loop fast, accurate, and safe.

> **No LLM, and no reinvented linters, in its own flow.** The engine hosts neither a model nor
> a from-scratch rule-checker. It *unifies, maps, indexes, and exposes* — specialized tools
> bring the per-language rule content; the consuming agent brings the judgment.

## What it does

A coding agent reaches for `commenter-cat` mid-task and stays in one loop:

```text
find ─▶ understand ─▶ judge ─▶ update ─▶ re-check ─┐
  ▲                                                 │
  └─────────────────────────────────────────────────┘
```

- **find** — `query` (keyword + semantic search) and `candidates` (a token-free blame-skew
  rot shortlist) return a ranked, bounded, paginated comment set — never a firehose.
- **understand** — `context` returns one comment plus its mapped code window, the unit on
  which the agent judges semantic rot.
- **rule-check** — `check` runs every provider and the native checks, normalized into one
  report across all four languages.
- **update** — `apply-edit` / `remove` land comment-only changes under the **safe-apply
  guarantee** and return the re-checked findings inline, so the loop closes in one call.

The split that organizes the substrate: **the engine owns the drift it can _prove_; the agent
owns the drift it must _judge_; the comment→code mapping is the handoff between them.**

## How it works

```text
walk + ignore ─▶ tree-sitter extract + classify ─▶ coalesce ─▶ map → bound_symbol
                                                                       │
   git blame + blame-skew rot candidates ───────────────────────────▶ normalize + index
                                                                       │
   ruff · eslint · shellcheck · gitleaks ──(normalized findings)─────▶ SQLite (FTS5 + vec)
                                                                       │
                                                            CLI verbs · MCP tools · JSONL
```

The native pass (walk, extract, map, enrich, index, embed) is pure Rust and ripgrep-class.
Rule *content* is delegated to external providers and unified into one `Finding` model. The
only non-Rust runtime is the eslint Node stack, fetched only when JS/TS is in scope.

## Installation

Requires **Rust 1.96+** (the workspace MSRV).

**From source:**

```sh
git clone https://github.com/msaidbilgehan/Commenter-Cat
cd Commenter-Cat
cargo install --path crates/commenter-cat-cli   # installs the `commenter-cat` binary
# or, without installing:
cargo build --release -p commenter-cat-cli      # → target/release/commenter-cat
```

**Prebuilt binaries** are published to [GitHub Releases](https://github.com/msaidbilgehan/Commenter-Cat/releases)
on each tagged release (`v*`) for the five tier-1 targets below. Each release artifact is a
single self-contained `commenter-cat` binary — `sqlite-vec` is statically linked and the ONNX embedding
model is fetched on first use, so there is no per-platform native side-artifact.

> Crates.io publishing is planned. Until then, install from source or grab a release binary.

### External providers

`commenter-cat`'s native pass (extraction, mapping, markers, rot candidates, search) runs with **no
external tools**. Deep per-language rules are delegated to providers that are **fetched and
cached at runtime, never bundled**:

| Language / job | Provider |
|---|---|
| Python | `ruff` |
| JavaScript / TypeScript | `eslint` + `eslint-plugin-jsdoc` / `eslint-plugin-tsdoc` |
| Shell | `shellcheck` |
| Secrets | `gitleaks` |

A missing provider degrades gracefully — that language's deep rules are skipped (warned,
non-fatal); the native pass still runs.

## Usage

```sh
commenter-cat check [PATHS...]              # run the analysis, render findings, persist the index
commenter-cat check src --format jsonl      # canonical machine-readable output
commenter-cat check --strict                # fail on ANY finding (CI gate)

commenter-cat candidates --limit 20         # the native rot + marker worklist, ranked & bounded
commenter-cat query "stale auth comment"    # search the persisted index (keyword + semantic)
commenter-cat context <COMMENT_ID>          # one comment + its findings
commenter-cat context <COMMENT_ID> --with-code   # ...plus the bound code span

commenter-cat apply-edit <COMMENT_ID> "# updated text"   # parse-invariant edit, re-checked inline
commenter-cat remove <COMMENT_ID>                         # remove a comment, re-checked inline

commenter-cat doctor                        # list the providers commenter-cat would run + their contract
commenter-cat install-hooks                 # install the non-fatal git cache-warmer hooks
commenter-cat mcp                           # serve the MCP protocol over stdio (the agent surface)
```

**Global flags** (apply to every verb): `--stats`, `--system-tools` (use tools on `PATH`
instead of the pinned toolchain), `--hermetic` (require the pinned toolchain), `--show-suppressed`.

`commenter-cat --help` lists every verb. Exit codes: `0` clean · `1` findings at/above the gate · `2`
an operational error.

> **Partially wired in the CLI:** `commenter-cat baseline accept|prune` is wired — it snapshots/prunes the
> committed baseline. `commenter-cat suppressions export` and `commenter-cat issues sync` parse but return an explicit
> "wire deliberately" message: the first mutates source (and depends on a suppression pass `check`
> does not yet apply), the second is network- and `gh`-backed and outward-facing. Both underlying
> engine modules exist and are exercised by tests.

### Output formats

`check` renders to **terminal** (default, grouped), **jsonl** (canonical, `schema_version`-tagged),
**sarif** (SARIF 2.1.0 for GitHub code-scanning), **markdown**, or **csv** (RFC 4180), via
`--format`.

### Safe-apply guarantee

`apply-edit` / `remove` are comment-only and enforced by the engine, never trusted to the agent:

1. **Parse-invariance** — after the edit, the file is re-parsed and the code-node tree must be
   byte-identical; if any code node changed, the edit **aborts**.
2. **Write-protection by kind** — behavior-bearing comments (`commenter-cat:*` and tool directives like
   `# noqa` / `// eslint-disable` / `# type: ignore`, shebangs, encoding declarations) require
   an explicit `--allow-significant` acknowledgment.

## Configuration

Configuration is discovered automatically, nearest-first:

1. `COMMENTER_CAT_*` environment variables (e.g. `COMMENTER_CAT_SEVERITY_FAIL_ON`, `COMMENTER_CAT_PROVIDERS_PYTHON`,
   `COMMENTER_CAT_OUTPUT_DEFAULT_FORMAT`) — highest precedence.
2. `commenter-cat.toml` files, walked up from the working directory (nearest wins).
3. The XDG global config at `$XDG_CONFIG_HOME/commenter-cat/config.toml`.

Sections include `[scan]`, `[providers]`, `[markers]`, `[severity]`, `[search]`, and `[output]`.

**Two project files sit beside the config (both versioned, both outside the cache):**

- `commenter-cat.baseline.toml` — the **committed** suppression baseline (lockfile-style,
  canonically ordered for minimal merge conflicts).

The per-project cache lives at `<repo_root>/.commenter-cat/` (two SQLite files: a
content-addressed `inputs.db` and a derived `index.db`). It is **gitignored** — local,
rebuildable, never committed. Shared/team truth is CI's job.

## Supported platforms

Tier-1 targets, built and released by CI (a missing build for any target fails the release):

| OS | Arch | Target triple |
|---|---|---|
| Linux | x86_64 | `x86_64-unknown-linux-gnu` (+ `-musl` static) |
| Linux | aarch64 | `aarch64-unknown-linux-gnu` |
| macOS | Apple Silicon | `aarch64-apple-darwin` |
| macOS | Intel | `x86_64-apple-darwin` |
| Windows | x86_64 | `x86_64-pc-windows-msvc` |

## Development

```sh
cargo build                                          # build the workspace
cargo test --workspace                               # full suite (unit + property + golden + contract + integration + MCP)
cargo test -p commenter-cat-engine -- --ignored                 # the real-ONNX embedding test (downloads a model)
cargo clippy --workspace --all-targets -- -D warnings   # lint gate (warnings are errors)
cargo fmt --all --check                              # format check
cargo bench -p commenter-cat-engine --bench native_pass         # native-pass benchmarks
```

### Project layout

```text
crates/
  commenter-cat-core/      # pure domain — errors, versioned contracts, config, Finding, Comment (no infrastructure)
  commenter-cat-engine/    # all infrastructure — walk, extract, map, git, storage, providers, MCP, render
  commenter-cat-cli/       # the thin `commenter-cat` binary (clap verb surface, 1:1 with the MCP tools)
Docs/
  Idea.md       # the full design decisions
  2026-06-15-build-commenter-cat-engine/   # the execution plan (STATUS.md = live state)
```

The crates enforce a strict layer discipline: `commenter-cat-core` imports no infrastructure; `commenter-cat-engine`
owns it and translates failures into `commenter_cat_core::CommenterCatError` at each adapter seam.

## License

[BSD-3-Clause](LICENSE) © Muhammed Said
