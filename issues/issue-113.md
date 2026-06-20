# Issue 113: Aggregate Acceleration

Milestone: V4 - Analytical Overlay
Area: Column Store Indexes
Status: Open
Priority: P3

## Requirement

Use column-batch metadata and precomputed segment summaries to accelerate supported aggregate queries without changing aggregate semantics.

## Functional Scope

- Maintain segment summaries for eligible column batches: row count, non-null count, min, max, sum where type-safe, and optional distinct hints.
- Planner selects aggregate acceleration for `count`, `sum`, `avg`, `min`, and `max` when all referenced fields and filters are covered by compatible column metadata.
- Executor combines segment summaries and only decodes rows/segments when filters or unsupported expressions require it.
- Preserve null handling, numeric conversion behavior, GROUP BY/HAVING semantics, ORDER BY, LIMIT, and OFFSET.
- Report accelerated segments, decoded fallback segments, and row-blob fallback through EXPLAIN/metrics.

## Non-Goals

- Do not approximate aggregate results.
- Do not accelerate aggregates involving user-defined functions, unsupported casts, or non-deterministic expressions.

## Acceptance Criteria

- Accelerated aggregate results are identical to row-executor aggregate results for covered queries.
- Unsupported aggregate/filter shapes fall back to existing execution.
- Segment summary maintenance survives writes, deletes, rebuilds, and restart hydration.
- EXPLAIN identifies aggregate acceleration and fallback reasons.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering count/sum/avg/min/max, nulls, filters, GROUP BY/HAVING, update/delete maintenance, restart hydration, and fallback.
- Include planner and integration tests.

## Closeout Steps

- Run the validation commands below.
- Run `cargo build --locked`.
- Run `cargo fmt --all -- --check`.
- Add benchmark evidence for aggregate acceleration.

## Validation

- `cargo test --test parser --quiet`
- `cargo test --test planner --quiet`
- `cargo test --test integration_sql --quiet`
- `cntryl-tools validate-tests -f tests/parser.rs`
