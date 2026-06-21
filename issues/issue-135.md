# Issue 135: Advanced Parallel Execution

Milestone: V5 - Verification & Advanced Execution
Area: Execution
Status: Open
Priority: P3

## Requirements

Introduce a bounded parallel execution framework for multi-operator plans beyond single parallel scan/scoring/aggregation features.

## Functional Scope

- Add exchange/partition/merge operators that can connect parallel scan, filter, projection, join, aggregate, sort, and scoring stages.
- Respect runtime worker limits, memory/spill budgets, query timeout, cancellation, and result limits across the whole pipeline.
- Preserve deterministic final results and stable tie-breaking across worker partitions.
- Propagate errors and cancellation exactly once while cleaning up all worker state.
- Report pipeline topology, workers, queue/backpressure metrics, and fallback through EXPLAIN/metrics.

## Non-Goals

- Do not require every physical operator to become parallel in this issue.
- Do not implement distributed execution across Cassie instances.

## Acceptance Criteria

- Parallel pipelines return identical results to single-worker execution for supported plans.
- Resource limits and cancellation are enforced across all workers and stages.
- Unsupported operators fall back to single-worker execution without changing results.
- EXPLAIN and metrics make the parallel topology observable.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering multi-stage parallel plans, deterministic merge, backpressure/resource limits, timeout/cancellation cleanup, error propagation, and fallback.
- Include planner and executor tests.

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
- `cargo test --locked --test planner_physical --test planner_logical --test planner_aggregates_sets`
- `cargo test --locked --test executor_parallel --test executor_query_sources --test executor_sort`
- `cargo test --locked --test integration_sql_joins --test integration_sql_join_plans --test integration_sql_aggregates`
- `cargo test --locked`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
