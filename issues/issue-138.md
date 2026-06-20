# Issue 138: Advanced Cardinality Estimation

Milestone: V5 - Verification & Advanced Execution
Area: Query Intelligence
Status: Open
Priority: P3

## Requirement

Improve planner cardinality estimates using persisted histograms, sketches, and correlation-aware statistics where available.

## Functional Scope

- Maintain advanced statistics for selected fields/indexes: value distribution buckets, null/missing counts, min/max, distinct estimates, and optional multi-field correlation stats.
- Persist statistics with schema/index epochs and rebuild them from row blobs or column batches.
- Use advanced estimates for filters, joins, GROUP BY, DISTINCT, search/vector prefilters, and index selection.
- Fall back to basic cardinality/default estimates when statistics are missing, stale, unsupported, or below confidence thresholds.
- Expose estimate source, confidence, and actual-vs-estimated diagnostics through EXPLAIN/metrics.

## Non-Goals

- Do not require exact statistics for query correctness.
- Do not implement automatic background sampling without explicit runtime controls.

## Acceptance Criteria

- Planner estimates improve for skewed values, null-heavy fields, range predicates, and correlated predicates compared with basic row counts.
- Statistics invalidate or partition across schema/index changes and database/collection boundaries.
- Missing/stale stats fall back deterministically.
- EXPLAIN shows which statistic source informed an estimate.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering histograms, distinct estimates, null/missing counts, correlated predicates, stale invalidation, fallback, and EXPLAIN diagnostics.
- Include planner, integration, and metrics tests.

## Closeout Steps

- Run the validation commands below.
- Validate any additional touched test file before closing.
- Run `cargo build --locked`.
- Run `cargo fmt --all -- --check`.
- Document statistic types, rebuild policy, and confidence defaults.

## Validation

- `cargo test --test planner --quiet`
- `cargo test --test integration_sql --quiet`
- `cargo test --test metrics --quiet`
- `cntryl-tools validate-tests -f tests/planner.rs`
- `cntryl-tools validate-tests -f tests/integration_sql.rs`
- `cntryl-tools validate-tests -f tests/metrics.rs`
