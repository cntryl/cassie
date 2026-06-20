# Issue 144: Analytical Projections

Milestone: V5 - Verification & Advanced Execution
Area: Advanced Analytics
Status: Open
Priority: P3

## Requirement

Support analytical projection definitions that materialize query-shaped read models optimized for scans, grouping, and search/vector analytics.

## Functional Scope

- Build on materialized projections, projection versioning, column batches, rollups, and verification metadata.
- Allow analytical projections to declare source collections, selected/derived fields, partition fields, sort keys, column storage options, and refresh policy.
- Maintain analytical projection data from source rows and mark freshness/lag explicitly.
- Planner can route eligible analytical queries to projections only when fields, filters, aggregates, freshness, and correctness guarantees match.
- Expose projection metadata, freshness, selected use, and fallback through catalog views, EXPLAIN, and metrics.

## Non-Goals

- Do not make analytical projections required for query correctness.
- Do not support arbitrary external data sources or non-deterministic projection definitions.

## Acceptance Criteria

- Analytical projections build, refresh, hydrate, query, and drop correctly.
- Eligible queries return identical results through analytical projections and source execution.
- Stale or incompatible projections fall back to source execution with diagnostics.
- Verification metadata can confirm projection build integrity when available.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering projection definition, build/refresh, query routing, stale fallback, restart hydration, drop cleanup, verification integration, and metrics.
- Include planner, integration, and catalog tests.

## Closeout Steps

- Run the validation commands below.
- Validate any additional touched test file before closing.
- Run `cargo build --locked`.
- Run `cargo fmt --all -- --check`.
- Document analytical projection syntax and freshness rules.

## Validation

- `cargo test --test planner --quiet`
- `cargo test --test integration_sql --quiet`
- `cargo test --test metrics --quiet`
- `cntryl-tools validate-tests -f tests/planner.rs`
- `cntryl-tools validate-tests -f tests/integration_sql.rs`
- `cntryl-tools validate-tests -f tests/metrics.rs`
