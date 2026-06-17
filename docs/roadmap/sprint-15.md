# Sprint 15 - Relational SQL Expansion

Previous: [Sprint 14 - Transactions and Session Semantics](sprint-14.md)  
Next: [Sprint 16 - PostgreSQL Catalog and Introspection](sprint-16.md)

## Goal

Expand Cassie's PostgreSQL-inspired SQL dialect beyond single-source SELECT so common application, ORM, migration, and BI queries can run through the same deterministic planner and executor.

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

- Support inner joins and left joins for V1 row sources.
- Support subqueries in `FROM`, scalar subqueries where deterministic, `IN`, `EXISTS`, `BETWEEN`, `IS NULL`, and `IS NOT NULL`.
- Support aggregates, `GROUP BY`, `HAVING`, and deterministic aggregate result metadata.
- Support `DISTINCT`.
- Support set operations needed for practical clients: `UNION` and `UNION ALL`.
- Support casts with `CAST(...)` and PostgreSQL-style `::type` for V1 types.
- Support `ORDER BY ... NULLS FIRST` and `ORDER BY ... NULLS LAST`.
- Return explicit unsupported errors for outer join forms, window functions, lateral joins, advanced grouping sets, and unsupported set operations unless implemented.

## Acceptance Criteria

- Joins, subqueries, aggregates, grouping, distinct, union, casts, and null ordering parse, bind, plan, and execute deterministically.
- Planner preserves clear operator order for relational features.
- Executor produces stable rows and metadata across repeated runs.
- Unsupported relational SQL features fail deterministically.
- `cargo build` passes.
- `cargo clippy --all-targets --all-features -- -D warnings` passes.
- All touched tests pass `cntryl-tools validate-tests`.

## Tests

- `tests/parser.rs`: parse relational SQL forms and unsupported variants.
- `tests/planner.rs`: logical and physical plans for joins, subqueries, aggregates, and set operations.
- `tests/executor.rs`: deterministic execution for each relational feature.
- `tests/integration_sql.rs`: combined relational query scenarios against Midge-backed data.
- `tests/pgwire.rs`: representative relational queries through pgwire after real protocol support lands.

## Exit Gate

This sprint is complete when relational SQL expansion is covered by validator-clean tests, targeted relational tests pass, `cargo build` passes, and Clippy is clean with warnings denied.

