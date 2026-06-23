# Phase 09 Issue 06: Persisted Bucket-Native Time-Series Storage

Milestone: Production Depth And Operational Orchestration
Area: Analytics And Time Series
Status: Open
Priority: P1

## Goal

Implement the next time-series depth slice by adding persisted bucket-native index storage or by first documenting the exact persistent format if implementation cannot be safely completed in one slice.

## Dependencies

- Phase 08 time-series MVP baseline is complete.
- Row blobs remain the source of truth.
- Any persistent key-layout decision must be explicit before code changes.

## Requirements

- Define the v1 bucket-native key layout, value payload, migration/fallback behavior, and cleanup semantics before implementation.
- Maintain authoritative row-blob reads as correctness fallback.
- Keep insert, update, delete, retention, restart, and rollup interactions deterministic.
- Expose metrics and EXPLAIN diagnostics for bucket-native hits, scanned buckets, skipped buckets, and fallback reasons.
- Add manual benchmark ownership for the bucket-native path.

## Acceptance Criteria

- Supported timestamp range queries can use persisted bucket-native membership after restart.
- Mutations and retention maintain or safely invalidate bucket membership.
- Corrupt or missing bucket metadata falls back to row blobs with observable diagnostics.
- Docs clearly distinguish row-backed MVP behavior from bucket-native depth behavior.

## Implementation Plan

1. Audit existing time-series metadata, row-backed scan behavior, retention, rollup, and Midge key layouts.
2. If the persistent format is not already explicit enough, update this issue and docs with the v1 format before writing storage code.
3. Write failing mutation/restart/fallback tests first.
4. Implement bucket membership writes and reads through the existing Midge adapter; do not add a second storage abstraction.
5. Update metrics, EXPLAIN, docs, and benchmark references.

## Required Tests

- Time-series range read tests after restart.
- Mutation and retention tests for bucket membership correctness.
- Fallback tests for missing or stale bucket metadata.
- Bench compile check for affected time-series benchmark owners.
- `cntryl-tools validate-tests -f <touched test file>`.

## Validation

```sh
cargo build --locked
CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo test --locked
cargo fmt --all -- --check
cntryl-tools validate-tests -f <touched test file>
```

## Close-Out Steps

- Confirm row blobs remain authoritative.
- Confirm persistent key-layout documentation is present.
- Delete this issue file only after implementation, validation, documentation, and close-out checks are complete.
