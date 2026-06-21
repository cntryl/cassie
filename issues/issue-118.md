# Issue 118: Time-Series Indexes

Milestone: V4 - Analytical Overlay
Area: Time Series
Status: Open
Priority: P3

## Requirements

Support time-series indexes that accelerate timestamp range predicates and bucketed analytical queries while keeping row blobs authoritative.

## Functional Scope

- Add parser/binder/catalog support for a time-series index over a timestamp field with optional bucket width and partition fields.
- Store index keys ordered by collection, partition values, bucket/range start, timestamp, and row id.
- Maintain index entries on ingest, SQL writes, updates, deletes, rebuild, restart hydration, rename, and drop.
- Planner selects time-series indexes for timestamp range filters, bucket predicates, retention enforcement, and eligible rollup refreshes.
- Expose index usage, buckets scanned/skipped, and fallback through EXPLAIN/metrics.

## Non-Goals

- Do not implement distributed partition movement or retention policy enforcement in this issue.
- Do not require time-series indexes for correctness.

## Acceptance Criteria

- Timestamp range queries using time-series indexes return identical rows and ordering to row-scan execution.
- Bucketed queries scan only relevant buckets when possible.
- Index maintenance handles rows that change timestamp or partition fields.
- Restart and rebuild preserve index metadata and entries.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering index creation, range query planning, bucket pruning, partition fields, timestamp update/delete maintenance, restart hydration, rebuild, and fallback.
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
