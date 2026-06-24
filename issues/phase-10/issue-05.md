# Phase 10 Issue 05: Time-Series Analytics And Rollup Efficiency

## Status

Open.

## Goal

Optimize time-series, analytical storage, aggregation, and rollup bottlenecks ranked by Issue 01.

## Dependencies

- `issues/phase-10/issue-01.md` is complete.
- The Phase 10 report assigns time-series, analytics, or rollup bottlenecks to this issue.

## Implementation Plan

1. Pick only bottlenecks assigned to this issue by the Phase 10 report.
2. Preserve bucket-native time-series semantics, row-backed fallback behavior, rollup freshness, retention effects, and aggregate correctness.
3. Prefer bucket locality, segment pruning, aggregate acceleration, fewer row decodes, and bounded rollup refresh work over new storage layers.
4. Keep diagnostics explicit for bucket hits, fallback reasons, rollup rewrites, and analytical projection selection.
5. Update the Phase 10 report with before/after evidence.

## Acceptance Criteria

- Time-series retention and rollup freshness behavior are unchanged.
- Every optimized path has before/after benchmark evidence.

## Validation

Run in order:

```sh
cargo build --locked --bin cassie
CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo test --locked
cargo fmt --all -- --check
cntryl-tools validate-tests -f <touched-test-file>
cargo bench --locked --bench <touched-benchmark> --no-run
```

Run the actual touched benchmark before close-out.
