# Changelog

All notable changes to Commenter-Cat are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `commenter-cat` CLI with verbs `check`, `candidates`, `query`, `context`, `apply-edit`, `remove`, `strip`, `doctor`, `baseline`, `mcp`, `install-hooks`
- Multi-language comment extraction (Python, TypeScript, JavaScript, Shell) via tree-sitter
- Comment-to-code mapping binding each comment to the symbol it annotates
- Comment kind classification (line, block, docstring, shebang, license, encoding-decl, directive)
- Unified `Finding` model normalizing ruff, eslint, shellcheck, and gitleaks output into one schema
- Manifest-first provider platform (declarative TOML adapters) with eslint as the Tier-2 native exception
- `comment_scoped` provider capability: a tool's findings count only when they sit inside an extracted comment span. gitleaks opts in so a secret surfaces only when it is *in a comment* (a key in a `# TODO`, a token in commented-out code) — hits in code or build artifacts (`target/`) are dropped at fusion and never surfaced as unattached, and the run-state is recomputed from survivors so an all-code run reads `EMPTY` instead of a misleading `SUCCESS`
- Native blame-skew rot candidates and cross-language marker triage (TODO/FIXME/HACK/...)
- Native **silent-rot detectors** — five deterministic, agent-judged detectors (`origin = native`, `fix = agent_only`) that catch a comment which reads like good documentation while making a factual claim about its bound code that is no longer true: reference-liveness (`rot_ref`), docstring↔signature contract (`rot_signature`, the `timeout`/`timeout_s` catch), path/identifier existence (`rot_path`), git-drift (`rot_drift`, blame-skew with an age threshold), and embedding-gated semantic contradiction (`rot_semantic`). A comment-intent classifier (directive / explanatory-note / log-level-reference / doc-contract) gates them so they fire only on checkable claims and quiet the `NOTE`/`WARNING` log-level false positives. Configured via a `[rot]` section (per-detector toggles + thresholds); the four structural detectors default on, the noisier semantic one defaults off and runs only behind the structural detectors at high confidence. No new provider or linter — these reuse Commenter-Cat's comment→code binding, the git `BlameIndex`, and the on-device embedder
- Parse-invariant safe-apply: comment-only edits assert the code tree is byte-identical or abort
- Write-protection by kind for behavior-bearing comments (directives, shebang, encoding-decl)
- `commenter-cat strip` — the scan-and-clean sweep: one walk over the tree, then per file a single splice proven code-invariant by the *same* leaf-token comparison the single-comment applier uses, written only once the whole batch verifies. Verification is per **file**, not per comment: re-parsing on every removal is quadratic exactly where a sweep lands hardest (a 1200-line module with 600 comments took ~1200 parses and ≈2s; it now takes ~10ms), and a batch that fails to verify falls back to the single-comment applier so the report names the offending comment instead of losing its neighbours. Source-mutating and repo-wide, so it **defaults to a dry-run plan** and rewrites only under `--apply`. Behavior-bearing comments (directive / shebang / encoding-decl) and license headers are protected — and *counted*, never silently dropped — unless surrendered by `--allow-significant` / `--strip-license`, with `--keep <kind>` adding protection. The blank line a removal leaves behind is tidied and the tidy re-verified against the same leaf-token stream (discarded wholesale if it differs; `--no-tidy` opts out). An unreadable file, a grammar failure, or a comment the applier refuses is a recorded skip, so one bad file never aborts the sweep
- Agent-facing MCP server (`commenter-cat mcp`) exposing the primitives 1:1 with the CLI verbs
- Token economy: actionable-first ranking with bounded, cursored results — never a firehose
- Two-layer per-project SQLite cache with FTS5 keyword and `sqlite-vec` semantic search
- Local ONNX embeddings (fastembed/ort), never shipping comments to an external API
- Filter-up suppression via `commenter-cat:*` directives and a committed Tier-2 baseline
- `commenter-cat suppressions export` materializes Commenter-Cat's suppression set into each tool's native directives (`# noqa`, `eslint-disable-next-line`, `# shellcheck disable`, `# gitleaks:allow`), merged per line and written through the parse-invariant applier; idempotent, Commenter-Cat-native findings skipped
- `commenter-cat issues sync` is bidirectional: forward-files flagged (marker) comments to the tracker and reverse-reconciles resolved (closed) issues by removing their marker comment through the parse-invariant applier (batched per file high→low). Defaults to a dry-run plan and mutates only under `--apply`; cross-run idempotency via a committed ledger (`commenter-cat.issues.toml`) keyed on Tier-4 comment identity, with the resolved link retired from the ledger on removal
- `commenter-cat check --show-suppressed` audit view, and `--stats` per-stage timing breakdown (walk / native / providers / fuse / index / total)
- Output renderers: JSONL (canonical), terminal, SARIF 2.1.0, Markdown, and CSV, with CI exit codes
- Git cache-warmer hooks (`commenter-cat install-hooks`) and CI integration with a two-key artifact cache
- `comment-to-issue` backend (GitHub via `gh`), idempotent on stable comment identity
- Layered configuration via `commenter-cat.toml`, an XDG global, and `COMMENTER_CAT_*` environment overrides
- One-command setup for Claude — `scripts/install.sh` builds + installs the `commenter-cat` binary and registers the `commenter-cat mcp` server (user scope by default, or `--scope project`), via the `claude` CLI when present else a safe, backed-up, atomic config merge; a committable `.mcp.json` wires the server for this repo

### Changed

- `commenter-cat check` applies the unified suppression pass (inline `commenter-cat:*` directives + committed baseline): suppressed findings remain in the index but are hidden from the default view and never gate CI, surfaced only under `--show-suppressed`
- The native pass (walk → extract → map → markers) fans out across CPU cores via `rayon`, preserving deterministic, sorted output
- Provider results are cached in the content-addressed `inputs.db` (keyed on input content + resolved tool version), so `commenter-cat check` never re-runs a provider on an unchanged tree — gitleaks no longer rescans every run; `--no-cache` bypasses the cache and `--stats` reports cache hits vs runs
- Standardized every identifier on `commenter-cat`, retiring the interim `cf` short handle — it abbreviated the original working name *comment-finder* (not Commenter-Cat) and collided with Cloud Foundry's CLI. The rename spans the `commenter-cat` binary, the `commenter-cat-core` / `commenter-cat-engine` / `commenter-cat-cli` crates, the `CommenterCat*` types, `COMMENTER_CAT_*` environment variables, the `commenter-cat:` inline directive, and the on-disk `.commenter-cat/` cache plus the committed `commenter-cat.toml` / `commenter-cat.baseline.toml` / `commenter-cat.issues.toml`

### Fixed

- Removing a Python docstring no longer fails outright. The applier required the re-parsed source to still hold a string at the edit site, which a *deletion* never does — so `remove` rejected every docstring, and a whole-tree `strip` could not touch the most common Python comment. The exclusion now applies only when a string is actually there, and the token-stream comparison alone judges the deletion; a break-out (`"""x"""; evil()`) still keeps its string, still shows `evil()` as extra tokens, and still aborts. Dropping a symbol's **only** docstring also still aborts — the emptied suite loses its indent/dedent tokens — and now says so in those words instead of the generic "would alter a code node"
- CI passes on Windows again — a repo-wide `.gitattributes` pins LF line endings, so `cargo fmt --all --check` no longer rejects the runner's CRLF-converted checkout; the format gate had been failing before clippy and the tests could run
- CI now exercises the real-ONNX embedding path in a dedicated job (the `#[ignore]`d test the fast offline suite skips), kept off the matrix's critical path; the all-MiniLM-L6-v2 model is cached across runs and only a transient HuggingFace *fetch* error is retried — a genuine assertion failure (dimensions, determinism, cosine ordering) still fails the build on the first attempt, never masked
- Providers no longer report a false `PARTIAL` (findings *unavailable*) on a clean scan — two cases surfaced by dogfooding `commenter-cat check` on a subdirectory: (1) a `{root}`-scoped tool (gitleaks) received a *relative* scan root that doubled against its `current_dir` (`sub/dir/sub/dir`) and fatal-exited with empty output → the root is now passed absolute; (2) a `{files}`-scoped tool (shellcheck/ruff) invoked with an empty file list (a scope holding none of its language) errored → such a provider is now `SKIPPED` (its language is off for this scope) rather than run empty
- Marker triage is **leading-position**, not a substring scan — a marker tags only when it *begins* a comment line (after that line's comment delimiters/furniture are stripped), followed by a boundary (`:`, an `(owner)`, whitespace, or end of line). Dogfooding flagged 54 "markers" of which ~45 were false positives: comments *about* WARNING-level logging and references to a TODO elsewhere — the marker word appearing mid-sentence. Those no longer tag, while `# TODO: fix`, ` * FIXME(me):`, and a leading `# NOTE …` still do
- `commenter-cat check` hands each provider **only the files in its declared languages**, so shellcheck no longer lints `.py`/`.ts`/`.js` as shell — dogfooding saw 793 of 795 `SC2148` ("add a shebang") findings fire on non-shell files, actively recommending a wrong fix. Providers declare `languages` in their manifest (`shellcheck = ["shell"]`, `ruff = ["python"]`; the eslint native provider declares TypeScript/JavaScript); the orchestrator narrows the file set before invoking, and a language with no files in scope `SKIP`s. An empty `languages` keeps the whole-tree default for `{root}` tools like gitleaks
- The MCP `check` tool returns a **bounded, ranked summary** instead of every full comment record — the old dump was ~11M chars on one line and blew the agent token ceiling, ironic for the agent surface that elsewhere promised "never a firehose." `check` now mirrors `candidates`/`query`: a severity histogram with comment/finding/unattached/suppressed counts, the per-provider run states, and an actionable-first `findings` slice paginated by `limit` (default 50) / `cursor` with `total`/`truncated` labels. The full records stay in the index for `query`/`context` to drill into

### Security

- Secret scanning covers the whole `commenter_cat_scope` universe — config files such as `.env` are scanned even though they carry no comments, while gitignored and excluded paths stay out of scope
- Local-first by default: zero network egress except two opt-in paths (`comment-to-issue`, CI SARIF upload)
- Provider manifests declare a command to spawn and carry no embedded code; provider binaries are version/hash-pinned

[Unreleased]: https://github.com/msaidbilgehan/Commenter-Cat/commits/master
