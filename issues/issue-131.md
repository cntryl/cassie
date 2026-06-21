# Issue 131: Full Column-Store Tables

Milestone: V5 - Verification & Advanced Execution
Area: Column Tables
Status: Open
Priority: P3

## Requirements

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

## Close-Out Steps

- Confirm every requirement and acceptance criterion above is implemented and covered by tests.
- Keep source, test, and benchmark files under 1,000 lines; split focused modules/tests before adding large blocks.
- Keep new code in the owning subsystem shown in `AGENTS.md` and `docs/module_organization.md`; do not introduce a second storage abstraction.
- Update docs/catalog/EXPLAIN/metrics references when user-visible behavior changes.
- Run the validation commands below in order, including `cargo build --locked` before tests.
- Run `cntryl-tools validate-tests -f <path>` for every touched test file.
- Delete this issue file only after implementation, validation, documentation, and close-out checks are complete.

## Validation

- `cargo build --locked`
- `cargo test --locked --test parser_cte_schema --test planner_logical --test planner_physical`
- `cargo test --locked --test executor_projection --test executor_query_sources --test executor_parallel`
- `cargo test --locked --test integration_sql_projection --test integration_sql_aggregates --test catalog_introspection`
- `cargo test --locked --test midge_row_blob_layout --test midge_metadata_stats`
- `cargo test --locked`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
