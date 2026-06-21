# Issue 113: Aggregate Acceleration

Milestone: V4 - Analytical Overlay
Area: Column Store Indexes
Status: Open
Priority: P3

## Requirements

Use column-batch metadata and precomputed segment summaries to accelerate supported aggregate queries without changing aggregate semantics.

## Functional Scope

- Maintain segment summaries for eligible column batches: row count, non-null count, min, max, sum where type-safe, and optional distinct hints.
- Planner selects aggregate acceleration for `count`, `sum`, `avg`, `min`, and `max` when all referenced fields and filters are covered by compatible column metadata.
- Executor combines segment summaries and only decodes rows/segments when filters or unsupported expressions require it.
- Preserve null handling, numeric conversion behavior, GROUP BY/HAVING semantics, ORDER BY, LIMIT, and OFFSET.
- Report accelerated segments, decoded fallback segments, and row-blob fallback through EXPLAIN/metrics.

## Non-Goals

- Do not approximate aggregate results.
- Do not accelerate aggregates involving user-defined functions, unsupported casts, or non-deterministic expressions.

## Acceptance Criteria

- Accelerated aggregate results are identical to row-executor aggregate results for covered queries.
- Unsupported aggregate/filter shapes fall back to existing execution.
- Segment summary maintenance survives writes, deletes, rebuilds, and restart hydration.
- EXPLAIN identifies aggregate acceleration and fallback reasons.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering count/sum/avg/min/max, nulls, filters, GROUP BY/HAVING, update/delete maintenance, restart hydration, and fallback.
- Include planner and integration tests.

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
- `cargo test --locked --test parser_indexes --test parser_cte_schema`
- `cargo test --locked --test planner_logical --test planner_physical --test planner_commands`
- `cargo test --locked --test integration_sql_projection --test integration_sql_aggregates --test integration_sql_ordering --test integration_sql_catalog`
- `cargo test --locked --test midge_metadata_stats --test midge_row_blob_layout --test metrics_runtime`
- `cargo test --locked`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
