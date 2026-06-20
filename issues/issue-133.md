# Issue 133: Hybrid Row/Column Planning

Milestone: V5 - Verification & Advanced Execution
Area: Column Tables
Status: Open
Priority: P3

## Requirement

Plan queries across row and column access paths, choosing the lowest safe combination per operator while preserving a single logical result.

## Functional Scope

- Extend planning to consider row scans, row indexes, column batches, column-store tables, and row materialization costs for eligible subplans.
- Insert explicit row/column conversion operators when a downstream operator requires a different representation.
- Use cost-informed planning, cardinality stats, and operator feedback when available; otherwise use deterministic defaults.
- Preserve row-level correctness for filters, joins, ordering, LIMIT/OFFSET, DML, and protocol output.
- Explain chosen row/column paths, conversion points, and fallback reasons.

## Non-Goals

- Do not mix storage representations in a way that bypasses Midge or duplicates truth without catalog metadata.
- Do not implement runtime operator switching here.

## Acceptance Criteria

- Hybrid plans return identical results to pure row plans for covered query shapes.
- Planner chooses column paths for column-covered analytical work and row paths for row-oriented lookup/DML.
- Conversion operators are explicit and metrics report materialization counts.
- Missing column metadata or unsupported expressions fall back deterministically.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering row-only, column-only, mixed row/column, conversion boundaries, cost preference, fallback, and EXPLAIN diagnostics.
- Include planner and executor tests.

## Closeout Steps

- Run the validation commands below.
- Run `cargo build --locked`.
- Run `cargo fmt --all -- --check`.
- Document planner representation choices and conversion rules.

## Validation

- `cargo test --test planner --quiet`
- `cargo test --test executor --quiet`
- `cntryl-tools validate-tests -f tests/planner.rs`
