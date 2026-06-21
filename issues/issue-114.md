# Issue 114: Scan Acceleration

Milestone: V4 - Analytical Overlay
Area: Column Store Indexes
Status: Open
Priority: P3

## Requirements

Use column batches to accelerate projection-pruned scans and simple analytical filters while preserving row-blob fallback.

## Functional Scope

- Planner selects column-batch scan when projected fields, filter fields, and order requirements are covered by compatible column batches.
- Executor reads only required columns and applies filters using column values, segment min/max/null metadata, and row-id reconciliation.
- Preserve sparse-field null behavior, casts, aliases, deterministic ordering, LIMIT/OFFSET, and error behavior.
- Fall back to row scans when expressions, functions, joins, missing batches, unsupported types, or incompatible segment versions require it.
- Report skipped segments, decoded columns, row-blob fetches, and fallback reasons through EXPLAIN/metrics.

## Non-Goals

- Do not make column scans mandatory for correctness.
- Do not implement full column-native execution for joins/aggregates here; those are later issues.

## Acceptance Criteria

- Column-batch scan results match row-scan results for covered projections and filters.
- Segment pruning reduces decoded rows/segments for supported range/equality filters.
- Fallback paths preserve results for unsupported query shapes.
- Restart and rebuild paths keep scan acceleration available.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering projection-only scans, equality/range filters, segment pruning, null/sparse fields, fallback, restart hydration, and rebuild.
- Include planner and integration tests with EXPLAIN assertions.

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
