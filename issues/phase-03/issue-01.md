# Phase 03 Issue 01: Time-Series Indexes

Milestone: Read-Model Analytics
Area: Time Series
Status: Open
Priority: P2

## Requirements

Support time-series indexes that accelerate timestamp range predicates and bucketed analytical queries while keeping row blobs authoritative.
This issue defines the time-ordered index contract used by later analytical projections and large-scale aggregation planning.

## Dependencies

- Depends on existing catalog/index metadata, row blob storage, `time_bucket`/rollup semantics, and retention metadata where present.
- Consumes phase 02 issue 05 operations diagnostics conventions for freshness, fallback, and metrics vocabulary.

## Handoff

- Provides ordered bucket/range access paths consumed by phase 03 issue 02 cost-informed planning, phase 03 issue 12 analytical projections, and phase 03 issue 13 large-scale aggregations.

## Functional Scope

- Add parser/binder/catalog support for a time-series index over a timestamp field with optional bucket width and partition fields.
- Store versioned index keys ordered by database, collection, index id/version, partition values, bucket/range start, timestamp, and row id.
- Define deterministic handling for missing, null, non-timestamp, and out-of-range timestamp values.
- Maintain index entries on ingest, SQL writes, updates, deletes, rebuild, restart hydration, rename, and drop.
- Planner selects time-series indexes for timestamp range filters, `time_bucket` predicates, retention candidate scans, eligible rollup refreshes, and compatible ordered scans.
- Expose index usage, index version, buckets scanned/skipped, partition pruning, stale/unavailable state, and fallback through EXPLAIN/metrics.

## Non-Goals

- Do not implement distributed partition movement or retention policy enforcement in this issue.
- Do not require time-series indexes for correctness.
- Do not make bucketed results approximate; row blobs remain authoritative for returned rows.

## Acceptance Criteria

- Timestamp range queries using time-series indexes return identical rows and ordering to row-scan execution.
- Bucketed queries scan only relevant buckets when possible.
- Index maintenance handles rows that change timestamp or partition fields.
- Restart and rebuild preserve index metadata and entries.
- Missing or stale index state falls back deterministically with diagnostics instead of returning partial results.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering index creation/options, range query planning, bucket pruning, partition fields, null/missing/non-timestamp values, timestamp update/delete maintenance, restart hydration, rebuild, stale metadata, and fallback.
- Include parser/planner/integration tests.

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
- `cargo test --locked --test scalar_functions --test parser_expressions --test parser_core`
- `cargo test --locked --test integration_sql_aggregates --test integration_sql_ordering --test integration_sql_predicates`
- `cargo test --locked --test catalog_introspection --test metrics_runtime`
- `cargo test --locked`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
