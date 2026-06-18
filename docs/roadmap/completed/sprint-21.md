# Sprint 21 - Relational Predicates and Scalar SQL

Previous: [Sprint 20 - Transaction Write Semantics](sprint-20.md)
Next: [Sprint 22 - Joins and FROM Subqueries](../sprint-22.md)

## Goal

Expand scalar SQL expressions and predicates needed by practical application queries without taking on joins or aggregates yet.

## Requirements

- Support `IN`, `EXISTS`, `BETWEEN`, `IS NULL`, and `IS NOT NULL`.
- Support deterministic casts with `CAST(...)` and PostgreSQL-style `::type` for current V1 types.
- Support `ORDER BY ... NULLS FIRST` and `ORDER BY ... NULLS LAST`.
- Keep unsupported scalar and predicate forms explicit.

## Acceptance Criteria

- New predicates parse, bind, plan, and execute deterministically.
- Casts use the same value semantics as storage and REST round trips.
- Unsupported forms return stable errors.
- `cargo build`, Clippy, targeted tests, and touched-test validation pass.

## Tests

- Parser/binder tests for scalar forms and unsupported variants.
- Planner/executor tests for deterministic predicate and cast behavior.
- Integration SQL tests combining predicates with existing filters, ordering, and limits.

## Exit Gate

This sprint is complete when scalar SQL expansion is validator-clean, targeted tests pass, `cargo build` passes, and Clippy is clean with warnings denied.
