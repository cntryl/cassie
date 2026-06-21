# Issue 116: Rollups

Milestone: V4 - Analytical Overlay
Area: Time Series
Status: Open
Priority: P3

## Requirements

Support materialized time-series rollups over bucketed source data for common aggregate queries.

## Functional Scope

- Add catalog metadata for rollup definitions: source collection, timestamp field, bucket expression, group keys, aggregate expressions, version, state, and refresh cursor.
- Provide a SQL/admin creation path for rollups using deterministic aggregate expressions over `time_bucket`.
- Build and refresh rollup rows from row blobs, and maintain them across inserts, updates, deletes, rebuild, restart hydration, source rename/drop, and rollup drop.
- Planner can rewrite eligible aggregate queries to rollup reads when bucket, group keys, filters, and aggregates match safely.
- Expose rollup metadata, freshness, lag, and selected rewrites through catalog views, EXPLAIN, and metrics.

## Non-Goals

- Do not support arbitrary continuous queries or user-defined aggregate functions.
- Do not return stale rollup results silently when freshness requirements are not met; fall back to source rows instead.

## Acceptance Criteria

- Rollup build, refresh, restart hydration, and drop are deterministic and idempotent.
- Eligible aggregate queries return the same results via rollup as via source-row execution.
- Ineligible or stale rollups fall back to source execution with observable diagnostics.
- Updates/deletes that move rows across buckets correct previous rollup rows.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering rollup creation, initial build, incremental refresh, update/delete movement, restart hydration, query rewrite, stale fallback, and drop cleanup.
- Include integration and catalog introspection tests.

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
- `cargo test --locked --test scalar_functions --test parser_expressions --test parser_core`
- `cargo test --locked --test integration_sql_aggregates --test integration_sql_ordering --test integration_sql_predicates`
- `cargo test --locked --test catalog_introspection --test metrics_runtime`
- `cargo test --locked`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
