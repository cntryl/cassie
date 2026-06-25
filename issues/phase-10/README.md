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
