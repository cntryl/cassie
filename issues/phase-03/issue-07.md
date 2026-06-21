# Phase 03 Issue 07: Hybrid Row/Column Planning

Milestone: Read-Model Performance
Area: Hybrid Planning
Status: Open
Priority: P2

## Requirements

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
- `cargo test --locked --test parser_cte_schema --test planner_logical --test planner_physical`
- `cargo test --locked --test executor_projection --test executor_query_sources --test executor_parallel`
- `cargo test --locked --test integration_sql_projection --test integration_sql_aggregates --test catalog_introspection`
- `cargo test --locked --test midge_row_blob_layout --test midge_metadata_stats`
- `cargo test --locked`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
