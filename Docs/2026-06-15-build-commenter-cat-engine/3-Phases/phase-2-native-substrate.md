---
plan_slug: 2026-06-15-build-commenter-cat-engine
phase: 2
name: native-substrate
goal: "Build the Rust-native hot path: file walk, tree-sitter extraction with comment-kind classification, block coalescing, deterministic comment-to-code mapping, marker extraction, git enrichment, and blame-skew rot candidates."
depends_on_phases: [1]
parallel_safe_with_phases: [3]
tasks:
  - id: "2.1"
    name: implement-file-walk
    action: "Create the file-walk module in cf-engine using the `ignore` crate: respect .gitignore/.ignore with an opt-out flag, apply [scan] include/exclude as the authoritative cf_scope, and add the generated-file header-sniff heuristic so minified/generated pseudo-comments are excluded."
    files: [crates/cf-engine/src/walk.rs, crates/cf-engine/src/lib.rs]
    depends_on: []
    parallel_safe: true
    validation: "cargo test -p cf-engine walk:: passes, covering gitignore respect, opt-out, and generated-file rejection on a fixture tree"
    notes: "Idea §3 + §5: cf_scope is authoritative; providers later veto within it (provider_filter(cf_scope)). The `ignore` crate is ripgrep's engine (§10)."
  - id: "2.2"
    name: implement-treesitter-extraction
    action: "Create the extraction module: load pinned tree-sitter grammars (python, typescript, javascript, bash), walk each file's tree for `comment` nodes, capture line range + byte offsets + raw text, and classify each comment kind (line, block, docstring, shebang, license, encoding-decl, directive) per Idea §3."
    files: [crates/cf-engine/src/extract/mod.rs, crates/cf-engine/src/extract/kind.rs, crates/cf-engine/src/extract/grammars.rs]
    depends_on: ["2.1"]
    parallel_safe: false
    validation: "cargo test -p cf-engine extract:: passes; an `insta` snapshot per grammar captures kind + ranges with zero string-vs-comment false positives"
    notes: "Idea §3: tree-sitter not regex. `directive` = cf:* control comments (never finding targets). shebang+license default-suppressed. CRLF byte-offset handling tested here (§10/§11)."
  - id: "2.3"
    name: implement-block-coalescing
    action: "Add block coalescing to the extract module: merge adjacent line-comment nodes (no blank line between) into one logical block before mapping; docstrings remain single nodes."
    files: [crates/cf-engine/src/extract/coalesce.rs, crates/cf-engine/src/extract/mod.rs]
    depends_on: ["2.2"]
    parallel_safe: false
    validation: "cargo test -p cf-engine extract::coalesce passes on a fixture with adjacent line comments + an interrupting blank line"
    notes: "Idea §3 Block coalescing. Same module dir as 2.2 → sequential."
  - id: "2.4"
    name: implement-comment-code-mapping
    action: "Create the mapping module producing bound_symbol + bound_node_range with NO scoring heuristic, using the three deterministic rules from Idea §3: doc comment bound by language standard (PEP 257 for Python, JSDoc/TSDoc adjacency for TS/JS), trailing comment on same-line code node, lead comment binding down to the next sibling with no blank line; orphan → enclosing scope only."
    files: [crates/cf-engine/src/map/mod.rs, crates/cf-engine/src/map/doc_binding.rs, crates/cf-engine/src/map/geometry.rs]
    depends_on: ["2.3"]
    parallel_safe: false
    validation: "cargo test -p cf-engine map:: passes; `insta` snapshots verify bound_symbol/bound_node_range for doc, trailing, lead, and orphan cases per language"
    notes: "Load-bearing for context, candidates, and attaching provider findings to a symbol (§3). Residual rule = blank-line adjacency. This is the engine's unique handoff between proven and judged drift."
  - id: "2.5"
    name: implement-marker-extraction
    action: "Create the marker module extracting native markers (TODO, FIXME, HACK, XXX, BUG, NOTE, DEPRECATED, WARNING, REVIEW + [markers].custom) from comment text, tagging each comment record; markers feed triage/search/blame-skew (format-checking is the provider's job, not here)."
    files: [crates/cf-engine/src/markers.rs, crates/cf-engine/src/lib.rs]
    depends_on: ["2.2"]
    parallel_safe: true
    validation: "cargo test -p cf-engine markers:: passes, covering builtin + custom markers and a TODO→FIXME distinction"
    notes: "Idea §3 Marker extraction (native). Custom markers from [markers].custom (§12). Does NOT validate `# TODO(owner):` form — that is ruff TD002/003 (§5)."
  - id: "2.6"
    name: implement-git-enrichment
    action: "Create the git-enrichment module using `gix`: resolve repo root (handling worktrees/submodules via --show-toplevel/--git-common-dir), run blame to join author/date/commit onto each comment, and expose changed-file sets via git diff-tree for hook/CI use."
    files: [crates/cf-engine/src/git/mod.rs, crates/cf-engine/src/git/blame.rs, crates/cf-engine/src/git/repo.rs]
    depends_on: ["2.4"]
    parallel_safe: false
    validation: "cargo test -p cf-engine git:: passes against a fixture repo with known blame history"
    notes: "Idea §3 Git enrichment + §7 changed-file sets. `gix`/gitoxide pure-Rust (§10). Mock only at the seam if needed, never the real repo in integration tests (§11)."
  - id: "2.7"
    name: implement-blame-skew-candidates
    action: "Create the blame-skew rot-candidate detector: flag a comment whose blame date predates its bound code's blame date (code changed after comment written), setting is_rot_candidate; produce the token-free shortlist for the agent to judge."
    files: [crates/cf-engine/src/rot.rs, crates/cf-engine/src/lib.rs]
    depends_on: ["2.6"]
    parallel_safe: false
    validation: "cargo test -p cf-engine rot:: passes on a fixture where comment-blame predates code-blame and a control where it does not"
    notes: "Idea §3 + §9: native, unique — no provider does this; needs the mapping (2.4) + git join (2.6). Feeds the `candidates` verb (§4a). A native Finding with category=rot_candidate."
---

# Phase 2: Native Substrate

## Goal
Build the Rust-native hot path that no existing tool provides: walk → extract → classify kind → coalesce → map to `bound_symbol` → extract markers → enrich with git → flag blame-skew rot candidates. This is the substrate the design calls "uniquely ours" (Idea §3). It depends only on Phase 1 primitives and runs in parallel with Phase 3 (finding model). The mapping task (2.4) is the load-bearing handoff between drift the engine can prove and drift the agent must judge.

## Tasks

### 2.1 — implement-file-walk
- **Action:** File walk via the `ignore` crate with gitignore respect, opt-out, authoritative cf_scope, and generated-file header sniff.
- **Files:** `crates/cf-engine/src/walk.rs`, `crates/cf-engine/src/lib.rs`
- **Depends on:** none (within phase; phase depends on P1)
- **Validation:** `cargo test -p cf-engine walk::` passes (gitignore, opt-out, generated rejection).

### 2.2 — implement-treesitter-extraction
- **Action:** Tree-sitter extraction of `comment` nodes across four pinned grammars with kind classification.
- **Files:** `crates/cf-engine/src/extract/{mod.rs,kind.rs,grammars.rs}`
- **Depends on:** 2.1
- **Validation:** `cargo test -p cf-engine extract::` + per-grammar `insta` snapshots; zero false positives.
- CRLF byte-offset coordinate handling is tested here (Windows support, §10/§11).

### 2.3 — implement-block-coalescing
- **Action:** Merge adjacent line-comment nodes into one logical block; docstrings stay single nodes.
- **Files:** `crates/cf-engine/src/extract/coalesce.rs`, `crates/cf-engine/src/extract/mod.rs`
- **Depends on:** 2.2 (shares `extract/` module)
- **Validation:** `cargo test -p cf-engine extract::coalesce` on adjacent-with-blank-line fixture.

### 2.4 — implement-comment-code-mapping
- **Action:** Deterministic comment→code mapping (no heuristic) via three language-standard rules → `bound_symbol` + `bound_node_range`.
- **Files:** `crates/cf-engine/src/map/{mod.rs,doc_binding.rs,geometry.rs}`
- **Depends on:** 2.3
- **Validation:** `cargo test -p cf-engine map::` + `insta` snapshots for doc/trailing/lead/orphan per language.

### 2.5 — implement-marker-extraction
- **Action:** Extract native markers (+ custom) from comment text and tag records.
- **Files:** `crates/cf-engine/src/markers.rs`, `crates/cf-engine/src/lib.rs`
- **Depends on:** 2.2 (needs comment records, not mapping) — parallel-safe with 2.3/2.4.
- **Validation:** `cargo test -p cf-engine markers::` (builtin + custom + TODO/FIXME distinction).

### 2.6 — implement-git-enrichment
- **Action:** `gix`-based repo-root resolution, blame join (author/date/commit), and changed-file sets.
- **Files:** `crates/cf-engine/src/git/{mod.rs,blame.rs,repo.rs}`
- **Depends on:** 2.4
- **Validation:** `cargo test -p cf-engine git::` against a fixture repo with known history.

### 2.7 — implement-blame-skew-candidates
- **Action:** Flag comments whose blame predates bound-code blame; set `is_rot_candidate`; produce the token-free shortlist.
- **Files:** `crates/cf-engine/src/rot.rs`, `crates/cf-engine/src/lib.rs`
- **Depends on:** 2.6
- **Validation:** `cargo test -p cf-engine rot::` on skewed + control fixtures.

## Phase Validation
`cargo test -p cf-engine` passes for walk, extract, map, markers, git, and rot modules; the native pass produces a complete comment record (path, hashes, kind, ranges, raw text, bound_symbol, bound_node_range, marker tags, git author/date/commit, is_rot_candidate) for a multi-language fixture tree — every field of the Idea §4 "Comment record" shape except `findings[]` (added in Phase 3+).
