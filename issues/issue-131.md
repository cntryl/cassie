# Issue 131: Full Column-Store Tables

Milestone: V5 - Verification & Advanced Execution
Area: Column Tables
Status: Open
Priority: P3

## Requirement

Support Midge-backed column-store table storage mode for analytical tables while preserving SQL/catalog compatibility.

## Functional Scope

- Add table metadata for storage mode: row-store, column-indexed row-store, or column-store table.
- Provide a SQL/catalog path to create column-store tables without introducing a second storage layer.
- Store columnar data, row ids, visibility/deletion markers, schema/version metadata, and optional row materialization data in Midge.
- Support INSERT, UPDATE, DELETE, SELECT, schema hydration, rename/drop, and catalog introspection for column-store tables.
- Materialize row-shaped results for pgwire/REST consumers with the same type/null/sparse behavior as row-store tables.

## Non-Goals

- Do not migrate existing row-store tables automatically.
- Do not bypass Midge or introduce an independent storage engine.

## Acceptance Criteria

- Column-store table creation, writes, reads, updates, deletes, restart hydration, rename, and drop work through existing SQL paths.
- Query results match equivalent row-store table behavior for supported types.
- Unsupported DDL/DML features fail clearly rather than partially writing data.
- EXPLAIN and catalog views identify column-store table storage mode.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering create/insert/select/update/delete, restart hydration, rename/drop, unsupported feature rejection, and catalog introspection.
- Include planner and executor tests.

## Closeout Steps

- Run the validation commands below.
- Run `cargo build --locked`.
- Run `cargo fmt --all -- --check`.
- Document storage mode syntax and compatibility limits.

## Validation

- `cargo test --test planner --quiet`
- `cargo test --test executor --quiet`
- `cntryl-tools validate-tests -f tests/planner.rs`
