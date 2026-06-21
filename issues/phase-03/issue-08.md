# Phase 03 Issue 08: Advanced Parallel Execution

Milestone: Read-Model Performance
Area: Execution
Status: Open
Priority: P2

## Requirements

Introduce a bounded parallel execution framework for multi-operator plans beyond single parallel scan/scoring/aggregation features.
Parallelism is a physical execution choice; every supported plan must keep the same final rows, errors, and ordering as single-worker execution.

## Dependencies

- Depends on existing executor cancellation/timeouts, parallel scan/scoring behavior, sort/aggregate semantics, and runtime limit configuration.
- Consumes phase 03 issue 02 cost-informed planning and phase 03 issue 07 hybrid planning where those are available.

## Handoff

- Provides exchange/partition/merge infrastructure consumed by phase 03 issue 09 vectorized aggregation and phase 03 issue 13 large-scale aggregations.

## Functional Scope

- Add exchange/partition/merge operators that can connect eligible parallel scan, filter, projection, join, aggregate, sort, and scoring stages.
- Respect runtime worker limits, memory/spill budgets, query timeout, cancellation, and result limits across the whole pipeline.
- Preserve deterministic final results and stable tie-breaking across worker partitions.
- Propagate errors and cancellation exactly once while cleaning up all worker state and temporary resources.
- Define single-worker fallback when an operator, expression, limit, or runtime setting makes a parallel segment unsafe.
- Report pipeline topology, workers, partitioning keys, queue/backpressure metrics, cancellation/error state, and fallback through EXPLAIN/metrics.

## Non-Goals

- Do not require every physical operator to become parallel in this issue.
- Do not implement distributed execution across Cassie instances.
- Do not make parallel execution the only path for large queries.

## Acceptance Criteria

- Parallel pipelines return identical results to single-worker execution for supported plans.
- Resource limits and cancellation are enforced across all workers and stages.
- Unsupported operators fall back to single-worker execution without changing results.
- EXPLAIN and metrics make the parallel topology observable.
- Worker failures do not leak partial result streams, background tasks, or temporary resources.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering multi-stage parallel plans, deterministic merge/tie order, worker-limit selection, backpressure/resource limits, timeout/cancellation cleanup, error propagation, temporary cleanup, and fallback.
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
