# Issue 081: Cardinality Tracking

Milestone: V2 - Query Performance
Area: Adaptive
Status: Open
Priority: P1

## Requirement

Track row and index cardinalities that the planner can use for deterministic cost estimates without changing query correctness.

## Functional Scope

- Maintain row-count statistics per collection across ingest, SQL INSERT/UPDATE/DELETE, collection rename/drop, and startup hydration.
- Maintain lightweight index cardinality statistics for scalar, composite, full-text, and vector indexes when those indexes exist.
- Persist statistics in the schema/control storage family with versioned keys so they survive restart and can be rebuilt from row blobs.
- Expose planner-visible estimates for scans, index scans, joins, search, vector, and aggregate planning.
- Surface statistics in metrics and EXPLAIN diagnostics without exposing unstable internal storage keys as public API.

## Non-Goals

- Do not require exact histograms, multi-column correlation, or adaptive re-planning in this issue.
- Do not make query results depend on the availability of cardinality statistics.

## Acceptance Criteria

- Row and index cardinalities update after inserts, updates that affect index membership, deletes, rebuilds, and restart hydration.
- Planner estimates are deterministic for repeated planning of the same catalog state.
- Missing or stale statistics degrade to conservative defaults and never cause incorrect results.
- Metrics expose enough counters to tell when statistics are read, written, rebuilt, or unavailable.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering row-count maintenance, index cardinality maintenance, restart hydration, collection drop/rename cleanup, and planner fallback when stats are missing.
- Include planner and metrics assertions.

## Closeout Steps

- Run the validation commands below.
- Run `cargo build --locked` because this touches catalog/runtime/planner types.
- Run `cargo fmt --all -- --check`.
- Document any new metrics fields.

## Validation

- `cargo test --test metrics --quiet`
- `cargo test --test planner --quiet`
- `cntryl-tools validate-tests -f tests/metrics.rs`
