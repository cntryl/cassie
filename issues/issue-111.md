# Issue 111: Column Batches

Milestone: V4 - Analytical Overlay
Area: Column Store Indexes
Status: Open
Priority: P3

## Requirements

Add optional column-batch index storage so analytical scans can read selected columns without decoding full row blobs.

## Functional Scope

- Support a SQL/catalog path for creating a column index over explicit fields, using row blobs as the source of truth.
- Store versioned column-batch metadata: collection, index name, schema epoch, fields, segment size, row-id range, null bitmap availability, and encoding version.
- Build column batches from row blobs and maintain them for SQL/REST writes, updates, deletes, rebuilds, collection rename/drop, and startup hydration.
- Preserve row-id mapping so column-batch reads can be reconciled with row blobs for fallback and correctness checks.
- Expose column-batch presence and use through catalog introspection, EXPLAIN, and metrics.

## Non-Goals

- Do not make column batches the default storage format.
- Do not require compression in this issue; compressed segments are issue 112.
- Do not support full column-store tables here; that is issue 131.

## Acceptance Criteria

- Column-batch metadata and payloads are created, persisted, hydrated, rebuilt, and dropped without changing row-blob contents.
- Column batches preserve nulls, missing sparse fields, type fidelity, and deterministic row order.
- Executor can read a column batch for covered analytical scans and fall back to row blobs when a batch is missing or incompatible.
- EXPLAIN/metrics identify column-batch scans and row-blob fallback.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering creation, persistence, hydration, rebuild, write maintenance, null/sparse fields, rename/drop cleanup, and fallback.
- Include parser/planner/integration tests for the chosen column-index syntax.

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
- `cargo test --locked --test parser_indexes --test parser_cte_schema`
- `cargo test --locked --test planner_logical --test planner_physical --test planner_commands`
- `cargo test --locked --test integration_sql_projection --test integration_sql_aggregates --test integration_sql_ordering --test integration_sql_catalog`
- `cargo test --locked --test midge_metadata_stats --test midge_row_blob_layout --test metrics_runtime`
- `cargo test --locked`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
