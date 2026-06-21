# Phase 03 Issue 10: Advanced Cardinality Estimation

Milestone: Read-Model Performance
Area: Query Intelligence
Status: Open
Priority: P2

## Requirements

Improve planner cardinality estimates using persisted histograms, sketches, and correlation-aware statistics where available.
Advanced statistics refine the cost model but must remain optional for correct query execution.

## Dependencies

- Depends on existing cardinality metadata, planner estimates, catalog epochs, and row blob/column-batch scan paths for statistics rebuild.
- Feeds phase 03 issue 02 cost-informed planning and phase 03 issue 03 index performance feedback.

## Handoff

- Provides statistics metadata, confidence scoring, and invalidation behavior consumed by phase 03 issue 02 cost-informed planning, phase 03 issue 07 hybrid planning, and phase 03 issue 13 large-scale aggregations.

## Functional Scope

- Maintain advanced statistics for selected fields/indexes: histogram buckets, null/missing counts, min/max, distinct estimates, heavy hitters where useful, and optional multi-field correlation stats.
- Persist statistics with database, collection, schema/index epochs, statistics version, sample/build metadata, and confidence.
- Rebuild statistics from row blobs or current column batches, and mark statistics stale when source coverage or epochs change.
- Use advanced estimates for filters, joins, GROUP BY, DISTINCT, ORDER BY/top-k, search/vector prefilters, and index selection.
- Fall back to basic cardinality/default estimates when statistics are missing, stale, unsupported, or below confidence thresholds.
- Expose estimate source, confidence, sample coverage, stale reason, and actual-vs-estimated diagnostics through EXPLAIN/metrics.

## Non-Goals

- Do not require exact statistics for query correctness.
- Do not implement automatic background sampling without explicit runtime controls.
- Do not persist raw field values in metrics labels or unbounded diagnostic keys.

## Acceptance Criteria

- Planner estimates improve for skewed values, null-heavy fields, range predicates, and correlated predicates compared with basic row counts.
- Statistics invalidate or partition across schema/index changes and database/collection boundaries.
- Missing/stale stats fall back deterministically.
- EXPLAIN shows which statistic source informed an estimate.
- Low-confidence or incompatible statistics are reported and ignored rather than treated as precise.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering histograms, heavy hitters where implemented, distinct estimates, null/missing counts, correlated predicates, stale invalidation, low-confidence fallback, database/collection partitioning, privacy/bounded labels, and EXPLAIN diagnostics.
- Include planner, integration, and metrics tests.

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
- `cargo test --locked --test planner_estimates --test planner_indexes --test planner_physical`
- `cargo test --locked --test metrics_adaptive --test metrics_feedback --test metrics_runtime --test metrics_search`
- `cargo test --locked --test integration_sql_predicates --test integration_sql_ordering --test integration_sql_projection`
- `cargo test --locked`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
