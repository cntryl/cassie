# Sprint 23 - Aggregates, DISTINCT, and Set Operations

Previous: [Sprint 22 - Joins and FROM Subqueries](sprint-22.md)
Next: [Sprint 24 - PostgreSQL Catalog Basics](../sprint-24.md)

## Goal

Add deterministic aggregate and set-query behavior needed by common application, migration, and BI-style queries.

## Requirements

- Support aggregates, `GROUP BY`, and `HAVING`.
- Support `DISTINCT`.
- Support `UNION` and `UNION ALL`.
- Return explicit unsupported errors for window functions, grouping sets, and unsupported set operations.

## Acceptance Criteria

- Aggregate, distinct, and set operation results are deterministic.
- Result metadata is stable and testable.
- Unsupported forms fail deterministically.
- `cargo build`, Clippy, targeted tests, and touched-test validation pass.

## Tests

- Parser, planner, executor, and integration tests for each supported feature.
- Metadata regression tests for aggregate and set-operation result columns.

## Exit Gate

This sprint is complete when aggregate and set-operation behavior is validator-clean, targeted tests pass, `cargo build` passes, and Clippy is clean with warnings denied.
