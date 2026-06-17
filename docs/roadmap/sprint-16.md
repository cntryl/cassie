# Sprint 16 - PostgreSQL Catalog and Introspection

Previous: [Sprint 15 - Relational SQL Expansion](sprint-15.md)  
Next: [Sprint 17 - PostgreSQL Type System and Wire Encodings](sprint-17.md)

## Goal

Provide enough PostgreSQL catalog and `information_schema` compatibility for practical clients, ORMs, migration tools, and BI tools to discover Cassie schemas without special adapters.

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

- Implement read-only virtual catalog views for practical subsets of `pg_catalog` and `information_schema`.
- Support metadata queries for namespaces, relations, attributes, types, indexes, constraints, functions, procedures, and server version.
- Support compatibility functions and statements commonly used by clients: `version()`, `current_schema()`, `current_database()`, `SHOW`, and selected `SET` no-op or validation behavior.
- Ensure catalog views are hydrated from Cassie's `Catalog` and Midge `cf0` metadata.
- Return deterministic empty rows or unsupported errors for PostgreSQL objects Cassie does not implement.
- Keep catalog query results stable enough for client compatibility tests.

## Acceptance Criteria

- Common metadata queries against `pg_catalog` and `information_schema` return stable results.
- Tables, columns, indexes, constraints, functions, and procedures are discoverable.
- `SHOW`, selected `SET`, `version()`, and current context functions behave deterministically.
- Unsupported catalog objects do not crash clients.
- `cargo build` passes.
- `cargo clippy --all-targets --all-features -- -D warnings` passes.
- All touched tests pass `cntryl-tools validate-tests`.

## Tests

- `tests/catalog_introspection.rs`: direct catalog query behavior.
- `tests/integration_sql.rs`: `pg_catalog` and `information_schema` metadata queries through SQL.
- `tests/pgwire.rs`: metadata queries over pgwire after real protocol support lands.
- Compatibility fixtures for ORM and migration metadata probes.

## Exit Gate

This sprint is complete when PostgreSQL catalog/introspection behavior is covered by validator-clean tests, client metadata probes pass, `cargo build` passes, and Clippy is clean with warnings denied. When the exit gates are green, move this file from `docs/roadmap/sprint-16.md` to `docs/roadmap/completed/sprint-16.md` and update the roadmap links to point at the completed copy.

