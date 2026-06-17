# Sprint 13 - SQL DML and Mutation Semantics

Previous: [Sprint 12 - Runtime Observability, Plan Cache, and Operational Controls](../completed/sprint-12.md)
Next: [Sprint 14 - Transactions and Session Semantics](sprint-14.md)

## Goal

Add PostgreSQL-compatible SQL mutation support so documents can be created, changed, and removed through the primary SQL interface instead of relying on REST-only write paths.

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

- Support `INSERT INTO ... VALUES ...` for V1 scalar, JSON, text, boolean, numeric, and vector fields.
- Support `INSERT INTO ... SELECT ...` when source and target schemas are compatible.
- Support `UPDATE ... SET ... WHERE ...`.
- Support `DELETE FROM ... WHERE ...`.
- Support `RETURNING` for inserted, updated, and deleted rows.
- Apply constraints, defaults, generated IDs, vector validation, and catalog type checks during SQL writes.
- Route all document mutation data through Midge `cf1`.
- Route schema and constraint metadata lookups through catalog data hydrated from `cf0`.
- Return explicit PostgreSQL-style unsupported errors for `ON CONFLICT`, writable CTE edge cases, multi-table updates, and advanced DML forms unless implemented in this sprint.

## Acceptance Criteria

- SQL inserts create retrievable documents in `cf1`.
- SQL updates mutate only rows matching the predicate.
- SQL deletes remove only rows matching the predicate.
- `RETURNING` returns deterministic rows and column metadata.
- SQL DML and REST writes enforce the same constraints and vector validation.
- Unsupported DML forms fail deterministically.
- `cargo build` passes.
- `cargo clippy --all-targets --all-features -- -D warnings` passes.
- All touched tests pass `cntryl-tools validate-tests`.

## Tests

- `tests/parser.rs`: parse INSERT, UPDATE, DELETE, and RETURNING forms.
- `tests/planner.rs`: plan DML into explicit mutation operations.
- `tests/executor.rs`: execute insert, update, delete, and returning semantics.
- `tests/integration_sql.rs`: verify SQL writes round-trip through Midge-backed reads.
- `tests/midge_cf_layout.rs`: prove SQL DML writes document payloads to `cf1`.
- `tests/rest.rs`: REST and SQL writes share validation behavior.

## Exit Gate

This sprint is complete when SQL mutation behavior is covered by validator-clean tests, targeted DML tests pass, `cargo build` passes, and Clippy is clean with warnings denied. When the exit gates are green, move this file from `docs/roadmap/sprint-13.md` to `docs/roadmap/completed/sprint-13.md` and update the roadmap links to point at the completed copy.
