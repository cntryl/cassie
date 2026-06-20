# Issue 114: Scan Acceleration

Milestone: V4 - Analytical Overlay
Area: Column Store Indexes
Status: Open
Priority: P3

## Requirement

Use column batches to accelerate projection-pruned scans and simple analytical filters while preserving row-blob fallback.

## Functional Scope

- Planner selects column-batch scan when projected fields, filter fields, and order requirements are covered by compatible column batches.
- Executor reads only required columns and applies filters using column values, segment min/max/null metadata, and row-id reconciliation.
- Preserve sparse-field null behavior, casts, aliases, deterministic ordering, LIMIT/OFFSET, and error behavior.
- Fall back to row scans when expressions, functions, joins, missing batches, unsupported types, or incompatible segment versions require it.
- Report skipped segments, decoded columns, row-blob fetches, and fallback reasons through EXPLAIN/metrics.

## Non-Goals

- Do not make column scans mandatory for correctness.
- Do not implement full column-native execution for joins/aggregates here; those are later issues.

## Acceptance Criteria

- Column-batch scan results match row-scan results for covered projections and filters.
- Segment pruning reduces decoded rows/segments for supported range/equality filters.
- Fallback paths preserve results for unsupported query shapes.
- Restart and rebuild paths keep scan acceleration available.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering projection-only scans, equality/range filters, segment pruning, null/sparse fields, fallback, restart hydration, and rebuild.
- Include planner and integration tests with EXPLAIN assertions.

## Closeout Steps

- Run the validation commands below.
- Run `cargo build --locked`.
- Run `cargo fmt --all -- --check`.
- Add benchmark evidence for scan acceleration.

## Validation

- `cargo test --test parser --quiet`
- `cargo test --test planner --quiet`
- `cargo test --test integration_sql --quiet`
- `cntryl-tools validate-tests -f tests/parser.rs`
