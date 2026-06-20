# Issue 132: Column-Native Execution Paths

Milestone: V5 - Verification & Advanced Execution
Area: Column Tables
Status: Open
Priority: P3

## Requirement

Execute eligible scan/filter/project/aggregate operations directly on columnar batches without first materializing full rows.

## Functional Scope

- Add physical operators for column-native scan, filter, projection, and simple aggregate paths.
- Keep row materialization only at boundaries that require row-shaped output, unsupported expressions, joins, or protocol encoding.
- Preserve null/missing semantics, casts, aliases, deterministic ordering, LIMIT/OFFSET, and errors.
- Fall back to row execution when expressions or data types are unsupported by column-native operators.
- Report column-native operator selection, decoded columns, row materialization count, and fallback through EXPLAIN/metrics.

## Non-Goals

- Do not implement vectorized joins or vectorized aggregation beyond simple column-native operations in this issue.
- Do not change user-visible result formats.

## Acceptance Criteria

- Column-native plans return identical results to row execution for supported scan/filter/project/aggregate shapes.
- Row materialization is avoided until required and is observable in metrics.
- Unsupported expressions fall back without changing results.
- Restart and mixed row/column storage states are handled deterministically.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering column-native filter/projection, aggregate, fallback, null/sparse behavior, row materialization boundary, and EXPLAIN diagnostics.
- Include planner and executor tests.

## Closeout Steps

- Run the validation commands below.
- Run `cargo build --locked`.
- Run `cargo fmt --all -- --check`.
- Add benchmark evidence for column-native scans.

## Validation

- `cargo test --test planner --quiet`
- `cargo test --test executor --quiet`
- `cntryl-tools validate-tests -f tests/planner.rs`
