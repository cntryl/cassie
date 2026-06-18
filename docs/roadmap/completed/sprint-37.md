# Sprint 37 - Common Scalar Functions

Previous: [Sprint 36 - Stored Procedure Execution and CALL Semantics](completed/sprint-36.md)
Next: [Sprint 38 - SQL Type Coverage and Metadata Fidelity](completed/sprint-38.md)

## Goal

Expand the built-in scalar function library so common SQL string and null-handling expressions work without requiring a custom UDF.

## Requirements

- Add common scalar helpers such as `length`/`len`, `lower`, `upper`, `substring`, `trim`, `concat`, `coalesce`, and `abs`.
- Keep existing search, vector, hybrid, and catalog helper functions working.
- Validate function arity and argument types deterministically.
- Keep UDF fallback behavior unchanged for user-defined functions.
- Surface function availability consistently through parser, binder, and executor paths.

## Acceptance Criteria

- The new scalar functions work in `SELECT`, `WHERE`, `ORDER BY`, and computed expressions.
- Built-in function errors are deterministic for bad arity or unsupported types.
- Existing aggregates (`count`, `sum`, `avg`, `min`, `max`) continue to behave as before.
- The sprint exits with touched-test validation, `cargo build`, and Clippy green.

## Tests

- Parser and executor tests for each added scalar function family.
- Query tests for function use in filters, projection, and sorting.
- Compatibility tests that cover the functions through pgwire.
- Regression tests to confirm UDF resolution still works after builtin expansion.

## Exit Gate

This sprint is complete when the scalar function suite is green, touched tests validate, `cargo build` passes, and Clippy is clean with warnings denied.
