# Sprint 22 - Joins and FROM Subqueries

Previous: [Sprint 21 - Relational Predicates and Scalar SQL](sprint-21.md)
Next: [Sprint 23 - Aggregates, DISTINCT, and Set Operations](sprint-23.md)

## Goal

Add V1 multi-source query support with deterministic inner joins, left joins, and subqueries in `FROM`.

## Requirements

- Support inner joins and left joins for V1 row sources.
- Support subqueries in `FROM` with deterministic aliases and column metadata.
- Plan joins with explicit logical and physical operators.
- Return explicit unsupported errors for outer join forms, lateral joins, and advanced join syntax not implemented here.

## Acceptance Criteria

- Joins and `FROM` subqueries parse, bind, plan, and execute deterministically.
- Result metadata is stable across repeated runs.
- Unsupported join forms fail deterministically.
- `cargo build`, Clippy, targeted tests, and touched-test validation pass.

## Tests

- Parser/binder tests for join and subquery syntax.
- Planner tests for operator order and source metadata.
- Executor/integration tests for inner join, left join, and subquery behavior.

## Exit Gate

This sprint is complete when join and `FROM` subquery behavior is validator-clean, targeted tests pass, `cargo build` passes, and Clippy is clean with warnings denied.
