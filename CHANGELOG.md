# Changelog

All notable changes to Commenter-Cat are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `commenter-cat` CLI with verbs `check`, `candidates`, `query`, `context`, `apply-edit`, `remove`, `doctor`, `baseline`, `mcp`, `install-hooks`
- Multi-language comment extraction (Python, TypeScript, JavaScript, Shell) via tree-sitter
- Comment-to-code mapping binding each comment to the symbol it annotates
- Comment kind classification (line, block, docstring, shebang, license, encoding-decl, directive)
- Unified `Finding` model normalizing ruff, eslint, shellcheck, and gitleaks output into one schema
- Manifest-first provider platform (declarative TOML adapters) with eslint as the Tier-2 native exception
- Native blame-skew rot candidates and cross-language marker triage (TODO/FIXME/HACK/...)
- Parse-invariant safe-apply: comment-only edits assert the code tree is byte-identical or abort
- Write-protection by kind for behavior-bearing comments (directives, shebang, encoding-decl)
- Agent-facing MCP server (`commenter-cat mcp`) exposing six primitives 1:1 with the CLI verbs
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

### Changed

- `commenter-cat check` applies the unified suppression pass (inline `commenter-cat:*` directives + committed baseline): suppressed findings remain in the index but are hidden from the default view and never gate CI, surfaced only under `--show-suppressed`
- The native pass (walk → extract → map → markers) fans out across CPU cores via `rayon`, preserving deterministic, sorted output
- Provider results are cached in the content-addressed `inputs.db` (keyed on input content + resolved tool version), so `commenter-cat check` never re-runs a provider on an unchanged tree — gitleaks no longer rescans every run; `--no-cache` bypasses the cache and `--stats` reports cache hits vs runs
- Standardized every identifier on `commenter-cat`, retiring the interim `cf` short handle — it abbreviated the original working name *comment-finder* (not Commenter-Cat) and collided with Cloud Foundry's CLI. The rename spans the `commenter-cat` binary, the `commenter-cat-core` / `commenter-cat-engine` / `commenter-cat-cli` crates, the `CommenterCat*` types, `COMMENTER_CAT_*` environment variables, the `commenter-cat:` inline directive, and the on-disk `.commenter-cat/` cache plus the committed `commenter-cat.toml` / `commenter-cat.baseline.toml` / `commenter-cat.issues.toml`

### Fixed

- CI passes on Windows again — a repo-wide `.gitattributes` pins LF line endings, so `cargo fmt --all --check` no longer rejects the runner's CRLF-converted checkout; the format gate had been failing before clippy and the tests could run
- CI now exercises the real-ONNX embedding path in a dedicated job (the `#[ignore]`d test the fast offline suite skips), kept off the matrix's critical path

### Security

- Secret scanning covers the whole `commenter_cat_scope` universe — config files such as `.env` are scanned even though they carry no comments, while gitignored and excluded paths stay out of scope
- Local-first by default: zero network egress except two opt-in paths (`comment-to-issue`, CI SARIF upload)
- Provider manifests declare a command to spawn and carry no embedded code; provider binaries are version/hash-pinned

[Unreleased]: https://github.com/msaidbilgehan/Commenter-Cat/commits/master
