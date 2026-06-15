# Test fixtures

Recorded, version-decoupled fixtures for the layered test suite (Idea §11).

## Layout

- `repos/` — small fixture repositories for the end-to-end integration tests
  (`integration_check.rs`). Each is a real tree with comments across languages;
  real tree-sitter and real SQLite run against them (the §11 principle: mock only
  at seams — never the parser or the DB).
- `../snapshots/` — `insta` golden files for `golden_extraction.rs`. They pin the
  per-grammar extraction + mapping output (`kind`, `bound_symbol`, ranges).
  Review changes with `cargo insta review`; accept with `cargo insta accept`.

## Recorded provider output

Per-provider JSON→`Finding[]` fixtures are **recorded** from a pinned tool
version and committed, so the normalization tests are decoupled from whatever
tool version happens to be on the test machine. Re-record only on a deliberate
provider bump, and review the diff.

## Principles

- **Deterministic.** No real clock, no network, no unseeded randomness.
- **Real systems where they tell the truth.** Blame/rot run against actual git
  repos built by the test harness; extraction runs against real grammars.
- **Seams only.** The network (issue trackers) and the clock are the only mocked
  boundaries.
