# Phase 07 Issue 02: Full Column-Store Tables

Milestone: Advanced Backlog
Area: Column Tables
Status: Open
Priority: P3

## Requirements

Support Midge-backed column-store table storage mode for analytical tables while preserving SQL/catalog compatibility.
This issue promotes columnar acceleration from derived execution support into an explicit table storage mode for read-heavy analytical read models.

## Dependencies

- Depends on phase 03 issue 06 for column-native execution paths.
- Depends on phase 03 issue 07 for hybrid row/column planning.
- Depends on phase 03 issue 12 for analytical projection semantics where column-store tables participate in projection workflows.
- Depends on phase 02 issue 05 for catalog, EXPLAIN, and metrics visibility.
- Depends on phase 05 issue 04 for persistent key/layout compatibility rules.
- Depends on phase 04 issue 07 for read-shape contracts and phase 06 issue 05 for diagnostics.

## Handoff

- Provides table-level columnar storage that phase 07 issue 04 vectorized joins and future analytical execution work can consume.

## Functional Scope

- Add table metadata for storage mode: row-store, column-indexed row-store, or column-store table.
- Provide a SQL/catalog path to create column-store tables without introducing a second storage layer.
- Store columnar data, row ids, visibility/deletion markers, schema/version metadata, and optional row materialization data in Midge.
- Reuse phase 05 layout compatibility rules for all persisted column-store keys and metadata.
- Support INSERT, UPDATE, DELETE, SELECT, schema hydration, rename/drop, and catalog introspection for column-store tables.
- Materialize row-shaped results for pgwire/REST consumers with the same type/null/sparse behavior as row-store tables.
- Preserve primary key/row id identity, deletion visibility, null/missing semantics, and schema evolution behavior across row-shaped and column-shaped access.
- Define unsupported table features up front and reject them before writing any partial Midge state.

## Non-Goals

- Do not migrate existing row-store tables automatically.
- Do not bypass Midge or introduce an independent storage engine.
- Do not make column-store tables the default storage mode.
- Do not add PostgreSQL storage-parameter parity beyond what Cassie needs for read-model workloads.

## Implementation Plan

### Step 1: Define storage-mode metadata

- Add a table-mode enum in catalog metadata (`row-store`, `column-indexed`, `column-store`).
- Define immutable mode transitions and explicit incompatible-mode rewrite rules.
- Add mode persistence semantics for rename/drop/restart recovery.

### Step 2: Bind DDL and catalog behavior

- Add DDL path support for explicit storage-mode creation and mode introspection.
- Ensure catalog hydration and introspection surfaces report storage mode and version.
- Add explicit rejection paths for unsupported mode combinations.

### Step 3: Add column-store key families

- Add a phase-ordered storage namespace for:
  - column values
  - row-id mapping
  - visibility/deletion markers
  - optional materialized row cache
- Reuse key-layout compatibility constants from phase 05 issue 04.

### Step 4: Implement column-store DML

- Add column-store INSERT/UPDATE/DELETE in terms of Midge writes that preserve identity and deletion semantics.
- Ensure restart hydration can replay metadata and visibility state without partial writes.
- Keep row-shaped execution output semantics identical where row-mode read paths consume results.

### Step 5: Extend read/write path selection

- Add planner/executor recognition for storage mode-specific projections.
- Select row-materialization or direct column-read paths only when semantics match required SQL behavior.
- Ensure unsupported predicate combinations fail with explicit error classes before data write.

### Step 6: Cleanup and observability

- Add catalog/drop cleanup to remove mode metadata and all associated column-storage families.
- Extend metrics/EXPLAIN/capability views to include storage mode and feature flags.
- Add config-level kill switch for experimental modes.

### Step 7: Validation and close-out

- Add `should_` fixture tests for create/write/read/update/delete/rename/drop.
- Add null/missing semantics tests and unsupported-feature rejection tests.
- Add restart hydration test and catalog view assertions.
- Add deterministic performance/regression checks for supported analytical patterns.

## Acceptance Criteria

- Column-store table creation, writes, reads, updates, deletes, restart hydration, rename, and drop work through existing SQL paths.
- Query results match equivalent row-store table behavior for supported types.
- Unsupported DDL/DML features fail clearly rather than partially writing data.
- EXPLAIN and catalog views identify column-store table storage mode.
- Row materialization through pgwire/REST preserves field order, type tags, nulls, and sparse/missing behavior.
- Dropping or renaming a column-store table cleans all related Midge metadata and column data.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering create/insert/select/update/delete, null/missing values, row materialization, restart hydration, rename/drop cleanup, unsupported feature rejection, and catalog introspection.
- Include planner and executor tests.

## Close-Out Steps

- Confirm every requirement and acceptance criterion above is implemented and covered by tests.
- Keep source, test, and benchmark files under 1,000 lines; split focused modules/tests before adding large blocks.
- Keep new code in the owning subsystem shown in `AGENTS.md` and `docs/module-organization.md`; do not introduce a second storage abstraction.
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
