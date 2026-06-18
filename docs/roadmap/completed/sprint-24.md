# Sprint 24 - PostgreSQL Catalog Basics

Previous: [Sprint 23 - Aggregates, DISTINCT, and Set Operations](sprint-23.md)
Next: [Sprint 25 - Catalog Compatibility Probes](../sprint-25.md)

## Goal

Provide basic read-only PostgreSQL catalog and `information_schema` views so clients can discover Cassie schemas.

## Requirements

- Add virtual catalog views for namespaces, relations, attributes, indexes, and constraints.
- Hydrate catalog rows from Cassie's in-memory catalog and Midge `cf0` metadata.
- Return deterministic empty rows for PostgreSQL objects Cassie does not implement when empty rows are safer than unsupported errors.

## Acceptance Criteria

- Tables, columns, indexes, and constraints are discoverable through SQL.
- Catalog rows remain stable across restart.
- `cargo build`, Clippy, targeted tests, and touched-test validation pass.

## Tests

- `tests/catalog_introspection.rs` for virtual catalog behavior.
- Integration SQL tests for representative `pg_catalog` and `information_schema` queries.

## Exit Gate

This sprint is complete when catalog basics are validator-clean, targeted tests pass, `cargo build` passes, and Clippy is clean with warnings denied.
