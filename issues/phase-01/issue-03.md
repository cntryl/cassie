# Phase 01 Issue 03: Materialized Projections

Milestone: Read-Model Core
Area: Projection Lifecycle
Status: Open
Priority: P0

## Requirements

Support persisted materialized projections derived from deterministic SELECT queries over source collections.

## Dependencies

- Depends on phase 01 issue 01 for projection checkpoint/freshness metadata.
- Uses phase 01 issue 02 replay semantics when source changes are applied through replay ingestion.

## Handoff

- Produces projection definitions, output collections, and build state consumed by phase 01 issue 04 versioning.

## Functional Scope

- Add SQL/catalog support for creating, listing, refreshing, and dropping materialized projections.
- Store projection definition, source collections, schema epoch, output schema, version, state, refresh cursor, and dependency metadata.
- Build projection rows from source row blobs and maintain or refresh them deterministically after source writes.
- Query materialized projections through the existing SQL path as read-only collections/views.
- Prevent DML against materialized projection outputs unless a future issue explicitly adds writable projections.
- Mark projections stale when source rows change and an incremental refresh cannot be proven deterministic.
- Reject non-deterministic projection definitions with deterministic errors.

## Non-Goals

- Do not support arbitrary non-deterministic functions, external side effects, or recursive materialized projections.
- Do not replace normal views or row blob source collections.
- Do not implement multiple active versions in this issue; that is phase 01 issue 04.
- Do not implement atomic active-version swaps in this issue; that is phase 01 issue 05.

## Acceptance Criteria

- Materialized projections create, build, refresh, hydrate after restart, and drop cleanly.
- Projection rows match the defining SELECT result for supported deterministic queries.
- Source writes either update projection state or mark it stale with observable diagnostics until refreshed.
- DML against materialized projection outputs is rejected with clear errors.
- Unsupported or non-deterministic definitions fail before any projection output is created.
- Drop cleanup removes projection metadata and output rows without deleting source collections.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering create/build/query, refresh after source writes, restart hydration, stale-state diagnostics, drop cleanup, and DML rejection.
- Include integration and catalog introspection tests.
- Include parser/binder tests for deterministic definition acceptance and non-deterministic definition rejection.

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
