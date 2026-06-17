# Sprint 07 - Schema Objects and DDL Compatibility

Previous: [Sprint 06 - Common Table Expressions](sprint-06.md)  
Next: [Sprint 08 - Indexes and Constraints](sprint-08.md)

## Goal

Add PostgreSQL-compatible schema object handling for Cassie's V1 catalog so practical clients, ORMs, migration tools, and SQL users can create, inspect, and evolve collections through SQL DDL instead of only REST.

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

- Support V1 DDL parsing, binding, planning, and execution for `CREATE TABLE`, `DROP TABLE`, `ALTER TABLE`, and `CREATE SCHEMA` compatibility where it maps to Cassie collections and catalog namespaces.
- Map PostgreSQL table terminology to Cassie collections without introducing a second storage abstraction.
- Persist schema object metadata in `cf0` and hydrate it into `Catalog` on startup.
- Support PostgreSQL-compatible type names for Cassie V1 scalar, JSON, text, boolean, numeric, and vector fields.
- Implement deterministic behavior for `IF EXISTS` and `IF NOT EXISTS`.
- Return explicit PostgreSQL-style unsupported errors for DDL features outside V1, including inheritance, partitioning, tablespaces, triggers, rules, and storage parameters.
- Provide enough catalog metadata for practical client introspection.
- Keep DDL operations idempotent where PostgreSQL syntax requests idempotence.

## Acceptance Criteria

- SQL DDL can create and drop Cassie collections.
- DDL-created schemas hydrate after restart.
- PostgreSQL-compatible type names map to Cassie types deterministically.
- `IF EXISTS` and `IF NOT EXISTS` behavior is deterministic.
- Unsupported DDL features produce stable PostgreSQL-style errors.
- Catalog introspection returns enough metadata for common clients to recognize tables and columns.
- `cargo build` passes.
- `cargo clippy --all-targets --all-features -- -D warnings` passes.
- All touched tests pass `cntryl-tools validate-tests`.

## Tests

- `tests/parser.rs`: parse supported DDL and reject unsupported DDL shapes.
- `tests/planner.rs`: plan DDL into explicit catalog operations.
- `tests/integration_sql.rs`: create, inspect, restart, and drop schema objects through SQL.
- `tests/midge_cf_layout.rs`: prove DDL metadata lands in `cf0` and user rows stay in `cf1`.
- `tests/pgwire.rs`: DDL command completion through pgwire after real protocol support lands.

## Exit Gate

This sprint is complete when schema object DDL is covered by validator-clean tests, targeted DDL tests pass, `cargo build` passes, and Clippy is clean with warnings denied.
