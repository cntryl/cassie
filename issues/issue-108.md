# Issue 108: Parallel Aggregation

Milestone: V3 - Advanced Query Features
Area: Execution
Status: Open
Priority: P2

## Requirement

Execute eligible aggregate queries using partial per-worker aggregation and deterministic final merge.

## Functional Scope

- Support parallel partial aggregation for `count`, `sum`, `avg`, `min`, and `max` over grouped and ungrouped queries where input expressions are deterministic.
- Partition input rows deterministically and merge partial aggregate states with stable group-key encoding.
- Preserve null handling, numeric type behavior, HAVING filters, DISTINCT interaction, ORDER BY, LIMIT, and OFFSET semantics.
- Keep single-worker fallback for unsupported aggregate expressions, user-defined functions, unstable ordering requirements, or worker limit of one.
- Report partial/final aggregate operators, worker counts, group counts, and fallback reason through EXPLAIN/metrics.

## Non-Goals

- Do not implement vectorized aggregation here; that is issue 136.
- Do not parallelize aggregate queries whose expressions have side effects or unsupported semantics.

## Acceptance Criteria

- Parallel aggregation returns identical rows, aggregate values, group ordering, and errors as single-worker aggregation.
- HAVING, DISTINCT, ORDER BY, LIMIT, and OFFSET still apply in the same logical order.
- Timeout/cancellation cleans up all worker state.
- EXPLAIN and metrics identify parallel aggregate execution.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering grouped and ungrouped aggregates, nulls, HAVING, DISTINCT, order/limit, fallback, timeout cleanup, and worker-limit behavior.
- Include planner and executor tests.

## Closeout Steps

- Run the validation commands below.
- Run `cargo build --locked`.
- Run `cargo fmt --all -- --check`.
- Add benchmark evidence for large aggregation workloads.

## Validation

- `cargo test --test planner --quiet`
- `cargo test --test executor --quiet`
- `cntryl-tools validate-tests -f tests/planner.rs`
- `cntryl-tools validate-tests -f tests/executor.rs`
