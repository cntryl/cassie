# Issue 145: Large-Scale Aggregations

Milestone: V5 - Verification & Advanced Execution
Area: Advanced Analytics
Status: Open
Priority: P3

## Requirements

Execute high-cardinality and high-volume aggregate workloads using column, vectorized, parallel, and spill-aware execution while preserving exact SQL results.

## Functional Scope

- Combine cost-informed planning, column-native scans, vectorized aggregation, parallel aggregation, and temp spill controls for large aggregate queries.
- Support grouped and ungrouped `count`, `sum`, `avg`, `min`, `max`, DISTINCT where implemented, HAVING, ORDER BY, LIMIT, and OFFSET.
- Use rollups or analytical projections only when freshness and query shape are compatible; otherwise compute from source data.
- Enforce memory, temp spill, timeout, and result limits with clear errors.
- Report selected acceleration paths, rows processed, groups created, spills, and fallback through EXPLAIN/metrics.

## Non-Goals

- Do not approximate aggregate results or use stale projections silently.
- Do not introduce a second storage engine.

## Acceptance Criteria

- Large aggregation results match exact source execution for supported query shapes.
- Memory/spill limits are respected and tested.
- Planner chooses available acceleration paths when safe and falls back when not.
- Metrics and EXPLAIN make resource use and selected paths observable.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering large grouped aggregation, high cardinality, spill behavior, timeout/limit errors, rollup/projection use, stale fallback, and deterministic output.
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
- `cargo test --locked --test planner_aggregates_sets --test planner_physical --test planner_estimates`
- `cargo test --locked --test integration_sql_aggregates --test integration_sql_fulltext_query --test integration_sql_hybrid_query`
- `cargo test --locked --test integration_sql_vector_indexes --test integration_sql_vector_query --test metrics_search --test metrics_adaptive`
- `cargo test --locked --test executor_parallel --test executor_vector_scoring --test rest_embeddings`
- `cargo test --locked`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
