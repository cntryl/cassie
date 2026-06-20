# Issue 085: INCLUDE Columns

Milestone: V2 - Query Performance
Area: Indexes
Status: Open
Priority: P1

## Requirement

Support PostgreSQL-style INCLUDE columns on scalar/composite indexes and persist included field payloads for covered-query execution.

## Functional Scope

- Parse and bind `CREATE INDEX name ON table USING btree (key1, key2) INCLUDE (field1, field2)`.
- Persist and hydrate include column metadata in the existing index catalog records.
- Reject duplicate include fields, include fields that duplicate key fields, unknown fields, and unsupported full-text/vector INCLUDE usage.
- Maintain versioned include payloads on SQL INSERT/UPDATE/DELETE, REST ingest, collection rename/drop, and index rebuild.
- Expose INCLUDE metadata through catalog introspection where index metadata is already exposed.

## Non-Goals

- Do not implement covered-query planning in this issue; that is issue 084.
- Do not support expressions or functions inside INCLUDE lists.

## Acceptance Criteria

- Parser, binder, executor command path, persistence, hydration, and drop paths all preserve include column metadata.
- Index payloads contain included values with correct type, null, and sparse-field behavior.
- Invalid INCLUDE statements return deterministic planner/parser errors.
- Existing scalar/composite indexes without INCLUDE continue to behave unchanged.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering valid syntax, duplicate/unknown rejection, metadata persistence after restart, payload maintenance on update/delete, and rebuild.
- Include parser, planner/catalog, and integration tests.

## Closeout Steps

- Run the validation commands below.
- Run `cargo build --locked` because this touches AST/catalog/storage types.
- Run `cargo fmt --all -- --check`.
- Update catalog/introspection docs if visible metadata changes.

## Validation

- `cargo test --test parser --quiet`
- `cargo test --test planner --quiet`
- `cargo test --test integration_sql --quiet`
- `cntryl-tools validate-tests -f tests/parser.rs`
- `cntryl-tools validate-tests -f tests/planner.rs`
