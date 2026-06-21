# Issue 132: Column-Native Execution Paths

Milestone: V5 - Verification & Advanced Execution
Area: Column Tables
Status: Open
Priority: P3

## Requirements

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

## Close-Out Steps

- Confirm every requirement and acceptance criterion above is implemented and covered by tests.
- Keep source, test, and benchmark files under 1,000 lines; split focused modules/tests before adding large blocks.
- Keep new code in the owning subsystem shown in `AGENTS.md` and `docs/module_organization.md`; do not introduce a second storage abstraction.
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
