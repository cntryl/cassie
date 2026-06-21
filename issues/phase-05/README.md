# Phase 05: Write Optimization

Phase 05 optimizes Cassie's write side for read-model workloads.

The goal is not generic write throughput.
The goal is to make projection replay, ingest, index maintenance, rebuilds, and metadata updates preserve Midge locality, reduce avoidable write amplification, and remain deterministic.

SQL, REST, and replay APIs are write interfaces.
They must not force Cassie into a generic per-row mutation model when the workload is actually a replay batch, projection rebuild, or index-maintained read-model update.

## Core Rule

A write path is not optimized because it commits correct state.
A write path is optimized only when it commits correct state through the intended Midge-efficient write pattern.

Each phase 05 change must define:

- required write-path behavior
- forbidden write-path behavior
- write amplification counters or benchmark evidence
- replay/rebuild correctness coverage
- cleanup behavior for timeout and cancellation

Each issue should include a concrete `Implementation Plan` section with the expected files/modules, TDD order, benchmark updates, and close-out sequence.
The goal is that implementation work is mostly mechanical once the issue is picked up.

## Write Pattern Categories

| Pattern | Purpose | Expected path |
| --- | --- | --- |
| Single projection mutation | Interactive projection-state correction or low-volume writes | direct row write plus required index/metadata deltas |
| Replay batch ingestion | Event-sourced projection catch-up | batch-local validation, idempotency checks, row/index writes grouped by projection |
| Duplicate replay skip | Idempotent event handling | duplicate ledger/checkpoint check with no row/index rewrite |
| Indexed mutation | Maintain scalar/composite/covering/search/vector access paths | compute field deltas once and write only affected index entries |
| Projection rebuild write | Build or refresh a derived read model | bulk-oriented buffering into inactive target/version namespace |
| Index rebuild write | Backfill an index from existing projection rows | streaming source scan with ordered index writes |
| Metadata/checkpoint update | Make replay/rebuild state observable | bounded metadata writes tied to batch or rebuild lifecycle |

## Phase Sequence

1. Write performance contracts: define supported write patterns and budgets before changing implementation.
2. Replay and ingest batching: reduce per-row overhead on the dominant write workflow.
3. Index maintenance batching: reduce secondary write amplification without weakening visibility.
4. Write-locality key layout: align keys and write ordering with Midge locality.
5. Bulk rebuild fast paths: separate rebuild/backfill behavior from interactive writes.
6. Write amplification diagnostics: make row, index, metadata, and rebuild write costs visible.

## Non-Goals

- No second storage abstraction.
- No external event-store dependency.
- No eventual index visibility for normal query paths.
- No benchmark-only shortcuts that bypass replay, freshness, verification, or lifecycle metadata.
- No optimization that makes rebuilds unverifiable or swaps less safe.
