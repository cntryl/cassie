# Issue 118: Time-Series Indexes

Milestone: V4 - Analytical Overlay
Area: Time Series
Status: Open
Priority: P3

## Requirement

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

## Closeout Steps

- Run the validation commands below.
- Run `cargo build --locked`.
- Run `cargo fmt --all -- --check`.
- Document syntax, key shape, and bucket semantics.

## Validation

- `cargo test --test scalar_functions --quiet`
- `cargo test --test integration_sql --quiet`
- `cntryl-tools validate-tests -f tests/scalar_functions.rs`
