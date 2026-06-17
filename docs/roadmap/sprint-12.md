# Sprint 12 - Runtime Observability, Plan Cache, and Operational Controls

Previous: [Sprint 11 - Vector and Hybrid Retrieval](sprint-11.md)  
Next: [Sprint 13 - SQL DML and Mutation Semantics](sprint-13.md)

## Goal

Add the runtime observability, shared plan cache, and operational controls Cassie needs before PostgreSQL wire clients begin relying on repeated query execution, prepared statements, long-lived sessions, and production deployment behavior.

## Invariants

- TDD first: add or update single-behavior tests before implementation.
- All touched tests use `should_` names plus `// Arrange`, `// Act`, `// Assert`.
- Validate touched tests with `cntryl-tools validate-tests -f <file>`.
- Keep Midge direct; no second storage abstraction.
- Preserve Midge family contract: `cf0` metadata/schema/config, `cf1` documents/data, `cf2` temp, `default` engine-reserved.
- Keep REST secondary and PostgreSQL wire primary.
- No Axum and no third-party SQL parser.
- Unsupported behavior returns deterministic `CassieError` or PostgreSQL-style wire errors.
- Each sprint exits only when targeted tests are green, touched tests pass `cntryl-tools validate-tests`, `cargo build` passes, and `cargo clippy --all-targets --all-features -- -D warnings` passes.
- Release sprints also run full `cargo test`.

## Requirements

- Add stable runtime metrics for query count, query latency, rows returned, errors by class, startup, shutdown, catalog hydration, and storage operation outcomes by Midge family.
- Add REST metrics for request count, latency, route, method, and status class.
- Add pgwire metrics for active sessions, authentication outcomes, protocol errors, simple-query executions, extended-query executions, prepared statements, and portals.
- Add search, vector, and hybrid metrics for count, latency, candidate count, and result count.
- Add plan cache metrics for hits, misses, invalidations, and evictions.
- Implement a shared plan cache keyed by normalized SQL, catalog version, parameter shape, and relevant execution mode.
- Keep per-session prepared statement state separate from the shared plan cache.
- Invalidate cached plans on DDL, index changes, constraint changes, UDF/procedure changes, catalog hydration, and relevant config changes.
- Add operational limits for query timeout, max result rows, CTE recursion depth, temp memory or `cf2` spill budget, and max plan cache entries.
- Ensure limit violations fail deterministically and map into stable `CassieError` or PostgreSQL-style wire errors.
- Preserve deterministic fallback to parse/bind/plan on plan cache miss or invalidation.

## Acceptance Criteria

- Metrics endpoint exposes stable, documented metric names.
- Query, storage, REST, pgwire, search, vector, hybrid, and plan cache metrics update under tests.
- Plan cache hit, miss, invalidation, and eviction behavior is deterministic.
- DDL and catalog mutations invalidate affected cached plans.
- Prepared statements can reuse valid cached plans without sharing session-local bind values.
- Query timeout, max row, recursion, temp, and cache-size limits fail deterministically.
- `cargo build` passes.
- `cargo clippy --all-targets --all-features -- -D warnings` passes.
- All touched tests pass `cntryl-tools validate-tests`.

## Tests

- `tests/metrics.rs`: stable metric names and metric increments for representative runtime paths.
- `tests/plan_cache.rs`: cache hit, miss, invalidation, eviction, and prepared-statement separation.
- `tests/integration_sql.rs`: DDL invalidates cached plans and repeated queries reuse valid plans.
- `tests/executor.rs`: query limits and CTE recursion limits fail deterministically.
- `tests/rest.rs`: metrics endpoint exposes runtime counters and REST status counters.
- `tests/pgwire.rs`: pgwire session and prepared-statement metrics after real protocol support lands.

## Exit Gate

This sprint is complete when observability, plan cache behavior, operational limits, and deterministic invalidation are covered by validator-clean tests, targeted runtime tests pass, `cargo build` passes, and Clippy is clean with warnings denied.
