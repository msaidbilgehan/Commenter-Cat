# Changelog

All notable changes to Commenter-Cat (`cf`) are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `cf` CLI with verbs `check`, `candidates`, `query`, `context`, `apply-edit`, `remove`, `doctor`, `baseline`, `mcp`, `install-hooks`
- Multi-language comment extraction (Python, TypeScript, JavaScript, Shell) via tree-sitter
- Comment-to-code mapping binding each comment to the symbol it annotates
- Comment kind classification (line, block, docstring, shebang, license, encoding-decl, directive)
- Unified `Finding` model normalizing ruff, eslint, shellcheck, and gitleaks output into one schema
- Manifest-first provider platform (declarative TOML adapters) with eslint as the Tier-2 native exception
- Native blame-skew rot candidates and cross-language marker triage (TODO/FIXME/HACK/...)
- Parse-invariant safe-apply: comment-only edits assert the code tree is byte-identical or abort
- Write-protection by kind for behavior-bearing comments (directives, shebang, encoding-decl)
- Agent-facing MCP server (`cf mcp`) exposing six primitives 1:1 with the CLI verbs
- Token economy: actionable-first ranking with bounded, cursored results — never a firehose
- Two-layer per-project SQLite cache with FTS5 keyword and `sqlite-vec` semantic search
- Local ONNX embeddings (fastembed/ort), never shipping comments to an external API
- Filter-up suppression via `cf:*` directives and a committed Tier-2 baseline
- Output renderers: JSONL (canonical), terminal, SARIF 2.1.0, Markdown, and CSV, with CI exit codes
- Git cache-warmer hooks (`cf install-hooks`) and CI integration with a two-key artifact cache
- `comment-to-issue` backend (GitHub via `gh`), idempotent on stable comment identity
- Layered configuration via `comment-finder.toml`, an XDG global, and `CF_*` environment overrides

### Security

- Secret scanning covers the whole `cf_scope` universe — config files such as `.env` are scanned even though they carry no comments, while gitignored and excluded paths stay out of scope
- Local-first by default: zero network egress except two opt-in paths (`comment-to-issue`, CI SARIF upload)
- Provider manifests declare a command to spawn and carry no embedded code; provider binaries are version/hash-pinned

[Unreleased]: https://github.com/msaidbilgehan/Commenter-Cat/commits/master
