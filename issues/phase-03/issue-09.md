# Phase 03 Issue 09: Vectorized Aggregation

Milestone: Read-Model Performance
Area: Execution
Status: Open
Priority: P2

## Requirements

Process aggregate inputs in columnar/vector batches for eligible numeric and timestamp aggregates while preserving exact aggregate semantics.
Vectorized aggregation accelerates the aggregate kernels only; SQL aggregate semantics remain owned by the existing aggregate executor contract.

## Dependencies

- Depends on phase 03 issue 06 for column-native input paths and phase 03 issue 08 for multi-stage parallel aggregation where parallel batches are used.
- Consumes existing aggregate semantics, numeric overflow behavior, null handling, and planner aggregate lowering.

## Handoff

- Provides typed aggregate kernels and metrics consumed by phase 03 issue 13 large-scale aggregations.

## Functional Scope

- Add versioned vectorized aggregate kernels for `count`, `sum`, `avg`, `min`, and `max` over supported primitive numeric, boolean where applicable, and timestamp/date types with null bitmaps.
- Use column-native or batch row data as input without per-value dynamic dispatch where the type and accumulator are known.
- Fall back to scalar aggregation for unsupported types, casts, functions, overflow-sensitive paths, or mixed dynamic values.
- Preserve null handling, numeric overflow/error behavior, GROUP BY/HAVING, DISTINCT where supported, ORDER BY, LIMIT, and OFFSET.
- Merge partial aggregate states deterministically when vectorized aggregation is combined with parallel execution.
- Report vectorized rows processed, fallback rows, kernel selected, partial merges, overflow/fallback reason, and elapsed time through EXPLAIN/metrics.

## Non-Goals

- Do not approximate aggregate results.
- Do not implement vectorized joins here; leave that for a future focused execution issue.
- Do not change SQL-visible aggregate types or overflow errors for speed.

## Acceptance Criteria

- Vectorized aggregate results match scalar aggregate results for supported types and query shapes.
- Fallback paths preserve results and errors for unsupported shapes.
- Null and overflow behavior are explicitly tested.
- Benchmarks or metrics show reduced per-row overhead for eligible aggregates.
- Parallel/vectorized partial-state merges are deterministic for grouped and ungrouped aggregates.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering count/sum/avg/min/max, grouped and ungrouped aggregation, nulls, timestamp/date aggregates, overflow/error cases, partial-state merge determinism, DISTINCT fallback or support, fallback, and EXPLAIN diagnostics.
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
