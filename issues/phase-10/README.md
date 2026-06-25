# Phase 10: Whole-System Performance Rebaseline

Phase 10 is the active performance gate after Phase 09 production-depth closure.

The goal is balanced speed and efficiency across Cassie's full single-node read-model surface using the existing `10k` and `100k` local fallback benchmark profiles. This phase is evidence-first: benchmark, explain, and runtime-metric evidence must rank bottlenecks before optimization work starts.

## Core Rule

Optimize only proven read-model bottlenecks. Do not broaden SQL semantics, add distributed execution, add a second storage abstraction, or claim production readiness from local benchmark output alone.

## Phase Sequence

1. Baseline evidence and bottleneck ranking.
2. Core planner and executor hot-path optimization.
3. Write, replay, rebuild, and verification efficiency.
4. Search, vector, hybrid, and graph retrieval efficiency.
5. Time-series, analytical, and rollup efficiency.
6. Pgwire, HTTP, startup, concurrency, and mixed-load efficiency.
7. Documentation, readiness, and capacity threshold reconciliation.

## Required Gates

- Phase 04 runtime and access-path contracts are archived in `docs/performance-contracts.md` and `issues/phase-04/README.md`.
- Phase 05 write-path contracts are archived in `docs/performance-contracts.md` and `issues/phase-05/README.md`.
- Phase 06 read optimization contracts are archived in `issues/phase-06/README.md`.
- Phase 07 advanced query contracts are archived in `issues/phase-07/README.md`.
- Phase 08 README-goal closure is archived in `issues/phase-08/README.md`.
- Phase 09 production-depth work is archived in `issues/phase-09/README.md`.

## Non-Goals

- No distributed SQL execution.
- No cross-node query planning.
- No replication, quorum reads, consensus, or automatic remote repair.
- No second storage abstraction above Midge.
- No production-ready promotion without deployment-profile evidence and explicit readiness updates.

## Archived Issue Summaries

- Issue 01, baseline evidence and bottleneck ranking, closed 2026-06-25. Baseline evidence was collected for tier-1 hot paths, tier-2 subsystems, mixed load, startup, pgwire, and HTTP. The remaining blockers were assigned to Issue 03 for projection refresh and replay catch-up, Issue 04 for graph 100k fixture setup, and Issue 05 for time-series 100k window scans. Harness fixes kept replay source identities stable, staged tier-3 fixture setup by benchmark section, and created time-series metadata before setup row loading.
- Issue 02, core planner and executor hot paths, closed 2026-06-25 as evidence-deferred. The Phase 10 baseline assigned no ranked core planner/executor bottleneck to this issue and explicitly deferred SQL parsing, binding, planning, core point reads, and pgwire simple reads unless later evidence changes. No SQL-visible behavior or execution internals were changed.
- Issue 03, write replay rebuild and verification efficiency, closed 2026-06-25. Replay duplicate-ledger checks were batched, existing-payload decodes were skipped for write batches without index families that need old values, and materialized projection refresh now writes fresh output rows plus row/range/root hashes in one pass. The `projection_refresh/10k` blocker moved from no completed sample / estimated `265.9 s` per iteration to focused diagnostic p50 `426.146 ms`, p95 `619.251 ms`; replay lag catch-up improved at both 10k and 100k while preserving idempotency, freshness, verification, and repair semantics.
- Issue 04, search vector hybrid and graph retrieval efficiency, closed 2026-06-25. The only Phase 10 retrieval blocker was graph 100k fixture setup; fresh graph fixture loading now writes row blobs, row hashes, and graph adjacency sidecars without generic per-row existence probes, and filtered tier-3 query benchmarks skip unmatched setup. `graph_expand_query/100k` moved from no benchmark label after more than 6 minutes to focused diagnostic p50 `8.542 us`, p95 `253.167 us`, while graph neighbor, expand, and shortest-path semantics remain covered.
- Issue 05, time-series analytics and rollup efficiency, closed 2026-06-25. Fresh time-series fixture loading now writes row blobs, row hashes, and bucket sidecars without generic per-row replacement probes, and time-series reads prune sidecar hits by timestamp range before scanning matching row blobs in one pass. `time_series_window_scan/100k` moved from no completed sample after more than 8 minutes to focused diagnostic p50 `282.283 ms`, p95 `295.009 ms`, while bucket-native reads, row-backed fallback, retention, and rollup freshness behavior remain covered.
