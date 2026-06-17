# Sprint 08 - Indexes and Constraints

Previous: [Sprint 07 - Schema Objects and DDL Compatibility](../sprint-07.md)  
Next: [Sprint 09 - UDFs and Stored Procedures](../sprint-09.md)

## Goal

Add V1 PostgreSQL-compatible index and constraint behavior so Cassie can enforce basic data integrity, expose useful catalog metadata, and give the planner index metadata it can use later.

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

- Support `CREATE INDEX`, `DROP INDEX`, and catalog persistence for scalar, full-text, and vector index metadata.
- Support `PRIMARY KEY`, `UNIQUE`, `NOT NULL`, `CHECK`, and default-value metadata for V1 write validation.
- Enforce constraints on REST ingest, SQL DDL-backed writes, and any future pgwire write path.
- Persist constraint and index metadata in `cf0`.
- Keep actual document data in `cf1`.
- Add planner visibility for index metadata without requiring every index to be used immediately.
- Return explicit PostgreSQL-style unsupported errors for foreign keys, exclusion constraints, partial indexes, expression indexes, and concurrent index operations unless implemented in the same sprint.
- Ensure constraint violations map to stable errors that pgwire can later encode with PostgreSQL-compatible SQLSTATE classes.

## Acceptance Criteria

- Index metadata can be created, listed through catalog paths, dropped, and rehydrated after restart.
- Constraint metadata can be created through DDL and enforced on writes.
- `PRIMARY KEY`, `UNIQUE`, `NOT NULL`, `CHECK`, and defaults have deterministic behavior.
- Constraint errors are stable and testable.
- Unsupported index and constraint features return explicit errors.
- `cargo build` passes.
- `cargo clippy --all-targets --all-features -- -D warnings` passes.
- All touched tests pass `cntryl-tools validate-tests`.

## Tests

- `tests/parser.rs`: parse index and constraint DDL.
- `tests/planner.rs`: expose index and constraint operations in logical plans.
- `tests/integration_sql.rs`: create/drop indexes and enforce constraints through SQL-created schema.
- `tests/rest.rs`: REST document writes honor constraints.
- `tests/midge_cf_layout.rs`: index and constraint metadata stays in `cf0`.
- `tests/vector_index_metadata.rs`: vector index metadata remains compatible with the broader index catalog.

## Exit Gate

This sprint is complete when index metadata, constraint enforcement, restart recovery, and unsupported-feature errors are covered by validator-clean tests, targeted tests pass, `cargo build` passes, and Clippy is clean with warnings denied.
