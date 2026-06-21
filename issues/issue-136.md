# Issue 136: Vectorized Aggregation

Milestone: V5 - Verification & Advanced Execution
Area: Execution
Status: Open
Priority: P3

## Requirements

Process aggregate inputs in columnar/vector batches for eligible numeric and timestamp aggregates while preserving exact aggregate semantics.

## Functional Scope

- Add vectorized aggregate kernels for `count`, `sum`, `avg`, `min`, and `max` over supported primitive types and null bitmaps.
- Use column-native or batch row data as input without per-value dynamic dispatch where the type is known.
- Fall back to scalar aggregation for unsupported types, casts, functions, overflow-sensitive paths, or mixed dynamic values.
- Preserve null handling, numeric overflow/error behavior, GROUP BY/HAVING, DISTINCT, ORDER BY, LIMIT, and OFFSET.
- Report vectorized rows processed, fallback rows, kernel selected, and elapsed time through EXPLAIN/metrics.

## Non-Goals

- Do not approximate aggregate results.
- Do not implement vectorized joins here; that is issue 137.

## Acceptance Criteria

- Vectorized aggregate results match scalar aggregate results for supported types and query shapes.
- Fallback paths preserve results and errors for unsupported shapes.
- Null and overflow behavior are explicitly tested.
- Benchmarks or metrics show reduced per-row overhead for eligible aggregates.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering count/sum/avg/min/max, grouped and ungrouped aggregation, nulls, overflow/error cases, fallback, and EXPLAIN diagnostics.
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
