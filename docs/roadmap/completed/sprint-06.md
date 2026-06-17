# Sprint 06 - Common Table Expressions

Previous: [Sprint 05 - Executor Semantics and Query Result Contract](sprint-05.md)  
Next: [Sprint 07 - Schema Objects and DDL Compatibility](../sprint-07.md)

## Goal

Make Common Table Expressions a first-class V1 PostgreSQL dialect feature across parser, binder, planner, optimizer, executor, and pgwire query execution.

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

- Support PostgreSQL-style `WITH` clauses for Cassie's V1 query surface.
- Support non-recursive CTEs, multiple CTE definitions, nested CTE scopes, CTE column alias lists, and references from later CTEs or the final query.
- Support `WITH RECURSIVE` with deterministic fixpoint execution, recursion depth protection, and stable output ordering when an outer `ORDER BY` is present.
- Bind CTE names and aliases before collection lookup so CTE references and real collection references are resolved deterministically.
- Reject duplicate CTE names, invalid alias counts, illegal forward references, and recursive CTEs that do not reference themselves in the recursive term.
- Carry parameter references through CTE bodies and the final query using the same bind values.
- Plan CTEs explicitly so materialized and recursive execution paths are visible in logical and physical plans.
- Execute CTEs through Cassie's existing row/value model without writing intermediate rows to `cf1`.
- Use `cf2` only for bounded temp execution state when materialization cannot remain in memory.
- Return PostgreSQL-style unsupported errors for writable CTE forms until Cassie adds the corresponding DML statements.

## Acceptance Criteria

- Non-recursive CTE queries parse, bind, plan, and execute.
- Multiple CTEs can reference earlier CTEs.
- Nested CTE scopes shadow outer names deterministically.
- CTE column alias lists control visible output column names.
- Recursive CTEs execute with deterministic termination and depth protection.
- Parameters inside CTE bodies and final queries use the same bind values.
- Unsupported writable CTEs return explicit PostgreSQL-style errors.
- `cargo build` passes.
- `cargo clippy --all-targets --all-features -- -D warnings` passes.
- All touched tests pass `cntryl-tools validate-tests`.

## Tests

- `tests/parser.rs`: parse `WITH`, multiple CTEs, column aliases, nested CTEs, and `WITH RECURSIVE`.
- `tests/planner.rs`: logical and physical plans expose CTE definitions, dependencies, materialization points, and recursive execution markers.
- `tests/executor.rs`: non-recursive CTE execution, CTE dependency ordering, alias visibility, and parameter propagation.
- `tests/executor.rs`: recursive CTE execution with termination, depth protection, and deterministic results.
- `tests/integration_sql.rs`: end-to-end CTE queries through `Cassie::execute_sql`.
- `tests/pgwire.rs`: parameterized CTE query through simple and extended PostgreSQL wire paths after pgwire support lands.

## Exit Gate

This sprint is complete when full PostgreSQL-style CTE support for Cassie's V1 query surface is covered by validator-clean tests, targeted CTE tests pass, `cargo build` passes, and Clippy is clean with warnings denied.
