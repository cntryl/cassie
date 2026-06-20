# Issue 136: Vectorized Aggregation

Milestone: V5 - Verification & Advanced Execution
Area: Execution
Status: Open
Priority: P3

## Requirement

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

## Closeout Steps

- Run the validation commands below.
- Run `cargo build --locked`.
- Run `cargo fmt --all -- --check`.
- Add benchmark evidence for vectorized aggregation.

## Validation

- `cargo test --test planner --quiet`
- `cargo test --test executor --quiet`
- `cntryl-tools validate-tests -f tests/planner.rs`
- `cntryl-tools validate-tests -f tests/executor.rs`
