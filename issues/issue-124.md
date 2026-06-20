# Issue 124: Index Performance Feedback

Milestone: V4 - Analytical Overlay
Area: Adaptive Planning
Status: Open
Priority: P3

## Requirement

Track observed index selectivity and cost so the planner can choose among competing indexes more accurately.

## Functional Scope

- Record per-index feedback for predicate shape, estimated rows, scanned index entries, fetched row blobs, returned rows, elapsed time, and fallback reason.
- Partition feedback by collection, index id/version, schema epoch, predicate shape, and database.
- Use feedback in cost-informed planning to prefer indexes that have lower observed cost for matching shapes.
- Invalidate feedback when index definition, schema epoch, collection, or database changes.
- Expose per-index feedback through metrics and EXPLAIN diagnostics without leaking bind values.

## Non-Goals

- Do not change row blob truth or index correctness semantics.
- Do not implement automatic index creation/drop decisions here.

## Acceptance Criteria

- Index feedback is captured after indexed query execution and ignored for full-scan fallback.
- Competing index selection can change when feedback consistently favors one index.
- Stale or missing feedback falls back to static estimates.
- Metrics show feedback reads, writes, invalidations, and selected index influence.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering feedback capture, competing index choice, stale invalidation, missing fallback, privacy/no bind values, and EXPLAIN diagnostics.
- Include planner and metrics tests.

## Closeout Steps

- Run the validation commands below.
- Run `cargo build --locked`.
- Run `cargo fmt --all -- --check`.
- Document feedback keys and invalidation rules.

## Validation

- `cargo test --test planner --quiet`
- `cargo test --test metrics --quiet`
- `cntryl-tools validate-tests -f tests/planner.rs`
