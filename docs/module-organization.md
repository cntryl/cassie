# Module Organization

Cassie keeps module boundaries small so feature work can be reviewed and tested surgically.

## Targets

- Source files should stay under 1,000 lines.
- Test files should stay under 1,000 lines and cover one behavior family.
- Legacy files over 1,000 lines need a written reason and a concrete split plan before related feature work grows them.
- Tests should live in subsystem-specific integration files with shared helpers under `tests/support/`.

## Executor

`src/executor/mod.rs` is the facade. Keep public exports there and place implementation in focused modules:

- `executor.rs`: execution entrypoints and orchestration glue.
- `aggregate_exec.rs`: aggregate execution.
- `window_exec.rs`: window function execution.
- `dml_command.rs`: SQL command and DDL execution.
- `dml.rs`: insert, update, and delete row mutations.
- `source.rs`: sources, joins, CTEs, set operations, distinct, and slicing.
- `scored.rs`: full-text, hybrid, and vector top-k paths.
- `projected_read.rs` and `scan.rs`: projected scans, covering indexes, pushdown reads, and column-batch read routing.
- `plan_inspection.rs`: query and expression feature detection.

## SQL

Keep parser, binder, AST, and function metadata separate. When parser or binder files grow, split by statement family rather than by syntax token.

Current oversized legacy source files that need dedicated extraction passes include `src/sql/parser.rs`, `src/app.rs`, `src/sql/binder.rs`, `src/midge/adapter.rs`, `src/executor/executor.rs`, `src/pgwire/connection.rs`, `src/executor/filter.rs`, `src/runtime.rs`, `src/executor/execution/scored.rs`, `src/midge/row_blob.rs`, and `src/planner/physical.rs`.

Storage-adjacent acceleration such as column batches belongs in focused `src/midge/adapter/*` modules and must keep Midge as the direct storage layer. Do not add another storage abstraction for analytical overlays.

## Tests

Integration tests should be named for the subsystem under test. Prefer adding a new focused test file over extending an already-large file.

Current oversized legacy test files that need dedicated split passes include `tests/parser.rs`, `tests/metrics.rs`, `tests/planner.rs`, `tests/midge_cf_layout.rs`, and `tests/pgwire_extended_query.rs`.

Use the file-size audit from `AGENTS.md` before starting broad feature work.
