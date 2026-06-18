# Commenter-Cat

[![CI](https://github.com/msaidbilgehan/Commenter-Cat/actions/workflows/ci.yml/badge.svg)](https://github.com/msaidbilgehan/Commenter-Cat/actions/workflows/ci.yml)
[![License: BSD-3-Clause](https://img.shields.io/badge/License-BSD--3--Clause-blue.svg)](LICENSE)

## In plain English

Code changes; the comments around it usually don't. A comment that once described the
code accurately drifts into a confident lie — the parameter it documents was renamed,
the file it points to was deleted, the behavior it promises no longer happens.

**Commenter-Cat finds those stale comments.** It reads your code, ties each comment to the
exact code it describes, and flags the ones that no longer match — plus the usual
documentation-lint issues from tools like ruff and eslint. Then it hands that list to a
coding agent (like Claude), which reads, judges, and safely rewrites or deletes each one
**without ever touching the code itself**.

Think of it as a spell-checker for the *truth* of your comments. The rest of this README
is the detailed version.

## Quick start

Get Commenter-Cat running inside Claude Code in one command:

```sh
git clone https://github.com/msaidbilgehan/Commenter-Cat
cd Commenter-Cat
scripts/install.sh        # build the binary + register the MCP server (all projects)
```

`install.sh` builds the `commenter-cat` binary and registers it as an MCP server for Claude
(idempotent; re-run any time). Reload Claude Code, run `/mcp` to confirm `commenter-cat` is
connected, then just ask:

> *"Use commenter-cat to find the stale comments in `src/` and fix the clear ones."*

Claude drives the whole **find → understand → judge → fix** loop through the MCP tools — see
[example prompts](#example-prompts) for more. Prefer the raw CLI, or want to scope it to one
repo? See [Installation](#installation) and [Use with Claude (MCP)](#use-with-claude-mcp).

## The detailed version

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

- **find** — `query` (keyword + semantic search) and `candidates` (a token-free native rot +
  marker worklist) return a ranked, bounded, paginated comment set — never a firehose.
- **understand** — `context` returns one comment plus its mapped code window, the unit on
  which the agent judges semantic rot.
- **rule-check** — `check` runs every provider and the native silent-rot detectors, normalized
  into one report across all four languages.
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

## Silent-rot detectors

No external linter catches *silent rot* — a comment that reads like good documentation while
making a claim about its bound code that is no longer true. Commenter-Cat does, natively, because
it already holds the comment→code binding, git blame, and an on-device embedder. Five
deterministic detectors run over each comment and the code it maps to:

| Detector | Rule id | Flags |
|---|---|---|
| Reference-liveness | `rot_ref` | a symbol named in the comment no longer exists in the repo |
| Docstring ↔ signature | `rot_signature` | the doc describes parameters the function no longer has |
| Path existence | `rot_path` | a file or path named in the comment is gone |
| Git-drift | `rot_drift` | the code moved out from under the comment (blame-skew past an age threshold) |
| Semantic contradiction | `rot_semantic` | comment and code disagree in meaning (embedding-gated) |

A comment-intent classifier gates them so they fire only on checkable claims, not on `NOTE:` /
`WARNING:` log-level mentions. Each emits a token-free shortlist (`origin = native`,
`fix = agent_only`) for the agent to judge — never a verdict. The four structural detectors are
**on by default**; the noisier semantic one is **opt-in** and runs only behind a clean structural
pass. A detector that cannot run (no git repo, embeddings unavailable, an unbound comment) is a
silent no-op, never a false finding. Toggle and tune each under `[rot]` in `commenter-cat.toml`.

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

commenter-cat baseline accept               # snapshot current findings into the committed baseline
commenter-cat suppressions export           # write Commenter-Cat's suppressions as native tool directives
commenter-cat issues sync                   # plan comment↔issue sync (dry-run; --apply to mutate)

commenter-cat doctor                        # list the providers commenter-cat would run + their contract
commenter-cat install-hooks                 # install the non-fatal git cache-warmer hooks
commenter-cat mcp                           # serve the MCP protocol over stdio (the agent surface)
```

**Global flags** (apply to every verb): `--stats`, `--system-tools` (use tools on `PATH`
instead of the pinned toolchain), `--hermetic` (require the pinned toolchain), `--show-suppressed`,
`--no-cache` (bypass the provider-result cache).

`commenter-cat --help` lists every verb. Exit codes: `0` clean · `1` findings at/above the gate · `2`
an operational error.

> **Every verb is wired end to end; the two outward-facing ones carry guardrails.**
> `commenter-cat suppressions export` mutates source — it writes each tool's native directives (`# noqa`,
> `eslint-disable-next-line`, `# shellcheck disable`, `# gitleaks:allow`) through the parse-invariant
> applier (a code-altering insert aborts) and is idempotent on the directive marker.
> `commenter-cat issues sync` is network- and `gh`-backed, so it **defaults to a dry-run plan** and mutates
> only under `--apply`, with cross-run idempotency from a committed ledger (`commenter-cat.issues.toml`).

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

## Use with Claude (MCP)

The MCP surface — driven live by a coding agent — is the product. Wire it into Claude
(Claude Code / Desktop) with the one-command installer:

```sh
scripts/install.sh                  # build + install the binary, register for all projects
scripts/install.sh --scope project  # register for this repo only (writes ./.mcp.json)
scripts/install.sh --no-build       # just (re)register the MCP server
```

It builds and installs the `commenter-cat` binary, then registers the `commenter-cat mcp`
server — via the `claude` CLI when present, otherwise a safe config merge (existing servers
preserved, original backed up, written atomically). The repo also ships a committable
`.mcp.json` that wires the server for this project:

```json
{ "mcpServers": { "commenter-cat": { "command": "commenter-cat", "args": ["mcp"] } } }
```

`commenter-cat mcp` is a **stdio** server: Claude spawns it per session (not a daemon), and
the per-project SQLite cache under `.commenter-cat/` carries state across spawns. After
registering, reload Claude and run `/mcp` to confirm `commenter-cat` is connected — the agent
then drives `query` · `context` · `check` · `candidates` · `apply-edit` · `remove`.

### Example prompts

Once `/mcp` shows `commenter-cat` connected, just ask in plain language — Claude picks the
right tools (`query` · `context` · `check` · `candidates` · `apply-edit` · `remove`) and only
ever edits comments, never the code:

- *"Find the stalest comments in this repo and fix the ones that are clearly wrong."*
- *"Run a commenter-cat check on `src/auth/` and group the comment findings by severity."*
- *"Triage the commenter-cat candidates list — for each, say keep / rewrite / delete with a one-line reason."*
- *"Any docstrings whose parameters no longer match the function signature? Update them to match."*
- *"Find comments that mention files or symbols that no longer exist, then correct or remove them."*
- *"Search the index for comments about rate limiting and tell me which ones are out of date."*
- *"Did I leave a secret in a `# TODO` anywhere? Check the comments for leaked tokens."*
- *"Accept the current findings as the baseline so CI only flags new comment rot from now on."*

## Configuration

Configuration is discovered automatically, nearest-first:

1. `COMMENTER_CAT_*` environment variables (e.g. `COMMENTER_CAT_SEVERITY_FAIL_ON`, `COMMENTER_CAT_PROVIDERS_PYTHON`,
   `COMMENTER_CAT_OUTPUT_DEFAULT_FORMAT`) — highest precedence.
2. `commenter-cat.toml` files, walked up from the working directory (nearest wins).
3. The XDG global config at `$XDG_CONFIG_HOME/commenter-cat/config.toml`.

Sections include `[scan]`, `[providers]`, `[markers]`, `[rot]`, `[severity]`, `[search]`, and `[output]`.

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
