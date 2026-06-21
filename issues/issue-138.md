# Issue 138: Advanced Cardinality Estimation

Milestone: V5 - Verification & Advanced Execution
Area: Query Intelligence
Status: Open
Priority: P3

## Requirements

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
- `cargo test --locked --test planner_estimates --test planner_indexes --test planner_physical`
- `cargo test --locked --test metrics_adaptive --test metrics_feedback --test metrics_runtime --test metrics_search`
- `cargo test --locked --test integration_sql_predicates --test integration_sql_ordering --test integration_sql_projection`
- `cargo test --locked`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
