---
plan_slug: 2026-06-15-build-commenter-cat-engine
section: execution
critical_path: ["1.1", "1.2", "1.4", "2.2", "2.4", "2.6", "4.2", "4.4", "4.7", "4.8", "6.1", "6.3", "6.4", "6.5", "6.7", "7.1", "7.2", "7.3", "7.6", "7.7", "8.1", "8.3", "8.4", "8.5", "10.1", "10.2", "10.3", "10.4"]
parallel_groups:
  - { after: "phase:0",  tasks: ["1.1"] }
  - { after: "task:1.1", tasks: ["1.2", "1.3"] }
  - { after: "phase:1",  tasks: ["2.1", "3.1"] }
  - { after: "task:2.2", tasks: ["2.3", "2.5"] }
  - { after: "task:3.1", tasks: ["3.2", "3.3"] }
  - { after: "phase:2-and-3", tasks: ["4.1"] }
  - { after: "task:4.1", tasks: ["4.2", "4.3", "4.5"] }
  - { after: "phase:4",  tasks: ["5.1", "6.1"] }
  - { after: "task:6.1", tasks: ["6.2", "6.3"] }
  - { after: "task:6.4", tasks: ["6.5", "6.6"] }
  - { after: "phase:5-and-6", tasks: ["7.1"] }
  - { after: "task:7.1", tasks: ["7.2", "7.4"] }
  - { after: "phase:7",  tasks: ["8.1"] }
  - { after: "task:8.1", tasks: ["8.2", "8.3"] }
  - { after: "phase:8",  tasks: ["9.1", "9.2", "10.1"] }
estimated_total_serial_tasks: 52
estimated_critical_path_tasks: 28
---

# Execution Graph

## Dependency DAG

```mermaid
graph TD
  %% Phase 1 — Foundation
  T1_1[1.1 scaffold-workspace] --> T1_2[1.2 error-hierarchy]
  T1_1 --> T1_3[1.3 versioned-contracts]
  T1_2 --> T1_4[1.4 config-model]
  T1_3 --> T1_4

  %% Phase 2 — Native substrate (after P1)
  T1_4 --> T2_1[2.1 walk]
  T2_1 --> T2_2[2.2 ts-extract+kind]
  T2_2 --> T2_3[2.3 coalesce]
  T2_3 --> T2_4[2.4 map bound_symbol]
  T2_2 --> T2_5[2.5 markers]
  T2_4 --> T2_6[2.6 git-enrich]
  T2_6 --> T2_7[2.7 blame-skew candidates]

  %% Phase 3 — Finding model (after P1, parallel with P2)
  T1_4 --> T3_1[3.1 Finding struct]
  T3_1 --> T3_2[3.2 severity]
  T3_1 --> T3_3[3.3 coordinates]
  T3_2 --> T3_4[3.4 dedup]
  T3_3 --> T3_4

  %% Phase 4 — Storage (after P2 + P3)
  T2_4 --> T4_1[4.1 inputs.db]
  T3_4 --> T4_1
  T4_1 --> T4_2[4.2 index.db]
  T4_1 --> T4_3[4.3 content-hash cache]
  T4_2 --> T4_4[4.4 FTS5]
  T4_1 --> T4_5[4.5 embeddings]
  T4_5 --> T4_6[4.6 sqlite-vec]
  T4_2 --> T4_6
  T4_4 --> T4_7[4.7 hybrid RRF]
  T4_6 --> T4_7
  T4_7 --> T4_8[4.8 rebuild-from-inputs]
  T4_3 --> T4_8

  %% Phase 5 — Identity (after P2 + P4, parallel with P6)
  T2_4 --> T5_1[5.1 fingerprint]
  T4_8 --> T5_1
  T5_1 --> T5_2[5.2 composite identity]
  T5_2 --> T5_3[5.3 tiered matching]

  %% Phase 6 — Providers (after P3 + P4)
  T3_4 --> T6_1[6.1 RuleProvider trait]
  T4_8 --> T6_1
  T6_1 --> T6_2[6.2 run-state]
  T6_1 --> T6_3[6.3 manifest provider]
  T6_3 --> T6_4[6.4 mapping+capabilities]
  T6_4 --> T6_5[6.5 builtin manifests]
  T6_4 --> T6_6[6.6 eslint native]
  T6_5 --> T6_7[6.7 pinning+doctor]
  T6_6 --> T6_7

  %% Phase 7 — Operations (after P5 + P6)
  T5_3 --> T7_1[7.1 commenter-cat check]
  T6_7 --> T7_1
  T7_1 --> T7_2[7.2 parse-invariant apply]
  T7_2 --> T7_3[7.3 write-protection]
  T7_1 --> T7_4[7.4 suppression filter-up]
  T7_4 --> T7_5[7.5 baseline file]
  T7_3 --> T7_6[7.6 fix/tighten]
  T7_5 --> T7_6
  T7_6 --> T7_7[7.7 suppression export]

  %% Phase 8 — Interfaces (after P7)
  T7_7 --> T8_1[8.1 CLI verbs]
  T8_1 --> T8_2[8.2 renderers]
  T8_1 --> T8_3[8.3 MCP surface]
  T8_3 --> T8_4[8.4 token economy]
  T8_2 --> T8_4
  T8_4 --> T8_5[8.5 round-trip re-check]

  %% Phase 9 — Adjacent (after P8, parallel with P10)
  T8_5 --> T9_1[9.1 comment-to-issue]
  T8_5 --> T9_2[9.2 git hooks]
  T9_2 --> T9_3[9.3 CI integration]

  %% Phase 10 — Distribution + tests (after P8, parallel with P9)
  T8_5 --> T10_1[10.1 property tests]
  T10_1 --> T10_2[10.2 golden+contract]
  T10_2 --> T10_3[10.3 integration+MCP]
  T10_3 --> T10_4[10.4 dist+OS-matrix CI]
```

## Critical Path
The longest dependency chain (28 tasks) threads the deepest serial work across every phase. Each step gates the next:

1. **1.1 → 1.2 → 1.4** — the workspace, then errors, then config gate all engine code.
2. **2.2 → 2.4 → 2.6** — extraction → mapping → git enrichment are strictly serial (each consumes the prior).
3. **4.2 → 4.4 → 4.7 → 4.8** — index schema → FTS5 → hybrid retrieval → rebuild; storage is the serial spine of the index.
4. **6.1 → 6.3 → 6.4 → 6.5 → 6.7** — provider trait → manifest → mapping → built-ins → pinning/doctor.
5. **7.1 → 7.2 → 7.3 → 7.6 → 7.7** — check → applier → write-protection → fix → export, all on the safe-write spine.
6. **8.1 → 8.3 → 8.4 → 8.5** — CLI verbs → MCP surface → token economy → round-trip (the product's loop).
7. **10.1 → 10.2 → 10.3 → 10.4** — property → golden/contract → integration/MCP → distribution; the proof-and-ship tail.

(Phase 5 identity feeds 7.1 but is shorter than the Phase-6 chain into 7.1, so it is not on the critical path. The cross-phase gates 4.2/4.4/4.7/4.8 and 6.x dominate the storage→provider→ops spine.)

## Parallel Groups
Front-loaded, wide-and-shallow where possible (see frontmatter `parallel_groups`):

- **After 1.1:** 1.2 and 1.3 run together (error hierarchy vs. version constants — different files).
- **After Phase 1:** **Phase 2 (native substrate) and Phase 3 (finding model) run fully in parallel** — the largest parallelization win; one lives in `commenter-cat-engine`, the other in `commenter-cat-core`.
- **Within Phase 2:** after 2.2, the marker extractor (2.5) runs alongside the coalesce→map chain (2.3→2.4).
- **Within Phase 4:** after 4.1, the index schema (4.2), content-hash cache (4.3), and embeddings (4.5) proceed in parallel before converging at 4.6/4.7.
- **After Phase 4:** **Phase 5 (identity) and Phase 6 (providers) run in parallel.**
- **Within Phase 6:** after 6.4, the dogfooded built-in manifests (6.5) and the eslint native provider (6.6) run in parallel before converging at 6.7.
- **Within Phase 7:** after 7.1, the applier chain (7.2→7.3) and the suppression chain (7.4→7.5) run in parallel before converging at 7.6.
- **Within Phase 8:** after 8.1, the renderers (8.2) and the MCP surface (8.3) run in parallel before converging at 8.4.
- **After Phase 8:** **Phase 9 (adjacent integrations) and Phase 10 (distribution + tests) run in parallel** — 9.1, 9.2, and 10.1 can all start at once.

## Atomic-Claim Protocol (embedded copy)

For parallel orchestration:

1. Worker reads `STATUS.md`.
2. Worker confirms task state is `ready` (all `depends_on` are `done`).
3. Worker writes `STATUS.md` with `state: claimed`, `worker_id`, `claimed_at`.
4. Worker re-reads `STATUS.md` immediately.
5. If `worker_id` matches own — claim succeeded; advance to `in_progress`.
6. If `worker_id` differs — claim lost; pick another `ready` task.

Best-effort coordination; markdown has no hard locking. For higher safety, route claims through a single orchestrator (workers dispatched via the `Task` tool never self-claim).

## Re-plan Triggers
Trigger a re-plan (edit phase files in a mutable state, reset to `queued`, append a `CHANGELOG.md` entry, re-run Step 5 self-validation) when:

- Any task enters `failed` and a retry needs a changed spec.
- An open question (Q1–Q4) resolves in a way that changes a task's `action`/`files`/`validation` (e.g., the chosen JSONPath library or embedding model forces a structural change).
- A discovered blocker (e.g., a Rust crate from §10 is unavailable or incompatible at MSRV 1.96) requires re-routing a dependency.
- A scope change request arrives (note: the Idea forbids scope reduction by phasing — additions land as new tasks, not as deferrals).
