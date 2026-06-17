# Sprint 05 - Executor Semantics and Query Result Contract

Previous: [Sprint 04 - Planner, Optimizer, and Physical Plan Determinism](completed/sprint-04.md)  
Next: [Sprint 06 - Common Table Expressions](sprint-06.md)

## Goal

Finalize deterministic SELECT execution semantics and the `QueryResult` contract so pgwire, REST, and native Cassie features all consume the same result shape.

## Invariants

- TDD first: add or update single-behavior tests before implementation.
- All touched tests use `should_` names plus `// Arrange`, `// Act`, `// Assert`.
- Validate touched tests with `cntryl-tools validate-tests -f <file>`.
- Keep Midge direct; no second storage abstraction.
- Preserve Midge family contract: `cf0` metadata/schema/config, `cf1` documents/data, `cf2` temp, `default` engine-reserved.
- Keep REST secondary and PostgreSQL wire primary.
- No Axum and no third-party SQL parser.
- Unsupported behavior returns deterministic `CassieError` or PostgreSQL-style wire errors.
- Each sprint exits only when targeted tests are green, touched tests pass `cntryl-tools validate-tests`, `cargo build` passes, and `cargo clippy --all-targets --all-features -- -D warnings` passes.
- Release sprints also run full `cargo test`.

## Requirements

- Enforce execution order: scan, filter, sort, project, offset, limit.
- Sort before projection when ordering expressions require unprojected fields or function aliases.
- Apply `OFFSET` before `LIMIT`.
- Use a stable tie-breaker for equal ordering keys.
- Keep omitted offset equivalent to `OFFSET 0`.
- Project missing columns as `Value::Null`.
- Evaluate supported functions in filters, projections, and order expressions.
- Keep `QueryResult.columns`, `QueryResult.rows`, and `QueryResult.command` stable for REST and PostgreSQL wire encoding.
- Map execution failures into `CassieError::Execution` or a more precise error where already defined.

## Acceptance Criteria

- SQL integration tests prove `ORDER BY` before `OFFSET/LIMIT`.
- Equal sort keys return deterministic ordering.
- Missing columns project as null.
- Function and column projections work together.
- Parameterized filters execute through the same path as literal filters.
- Query result metadata is deterministic for repeated executions.
- `cargo build` passes.
- `cargo clippy --all-targets --all-features -- -D warnings` passes.
- All touched tests pass `cntryl-tools validate-tests`.

## Tests

- `tests/executor.rs`: offset then limit semantics.
- `tests/executor.rs`: omitted offset equals explicit `OFFSET 0`.
- `tests/executor.rs`: stable sorting with equal keys.
- `tests/executor.rs`: projection of function and column values together.
- `tests/integration_sql.rs`: order, pagination, and restart-hydrated execution.
- Add regression tests for null projection and alias sorting before changing executor internals.

## Exit Gate

This sprint is complete when executor and SQL integration tests are validator-clean, `cargo test --test executor --test integration_sql` passes, `cargo build` passes, and Clippy is clean with warnings denied.
