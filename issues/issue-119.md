# Issue 119: Materialized Projections

Milestone: V4 - Analytical Overlay
Area: Materialization
Status: Open
Priority: P3

## Requirements

Support persisted materialized projections derived from deterministic SELECT queries over source collections.

## Functional Scope

- Add SQL/catalog support for creating, listing, refreshing, and dropping materialized projections.
- Store projection definition, source collections, schema epoch, output schema, version, state, refresh cursor, and dependency metadata.
- Build projection rows from source row blobs and maintain or refresh them deterministically after source writes.
- Query materialized projections through the existing SQL path as read-only collections/views.
- Prevent DML against materialized projection outputs unless a future issue explicitly adds writable projections.

## Non-Goals

- Do not support arbitrary non-deterministic functions, external side effects, or recursive materialized projections.
- Do not replace normal views or row blob source collections.

## Acceptance Criteria

- Materialized projections create, build, refresh, hydrate after restart, and drop cleanly.
- Projection rows match the defining SELECT result for supported deterministic queries.
- Source writes either update projection state or mark it stale with observable diagnostics until refreshed.
- DML against materialized projection outputs is rejected with clear errors.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering create/build/query, refresh after source writes, restart hydration, stale-state diagnostics, drop cleanup, and DML rejection.
- Include integration and catalog introspection tests.

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
- `cargo test --locked --test parser_cte_schema --test planner_commands --test planner_logical`
- `cargo test --locked --test integration_sql_catalog --test integration_sql_projection --test views`
- `cargo test --locked --test catalog_introspection --test midge_metadata_stats`
- `cargo test --locked`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
