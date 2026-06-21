# Module Organization

Cassie keeps module boundaries small so feature work can be reviewed and tested surgically.

## Targets

- Source files should stay under 1,500 lines when practical.
- Files over 2,000 lines need a written reason and a concrete split plan.
- Tests should live in subsystem-specific integration files with shared helpers under `tests/support/`.

## Executor

`src/executor/mod.rs` is the facade. Keep public exports there and place implementation in focused modules:

- `executor.rs`: execution entrypoints and orchestration glue.
- `aggregate_exec.rs`: aggregate and window execution.
- `dml.rs`: insert, update, delete, and command execution.
- `source.rs`: sources, joins, CTEs, set operations, distinct, and slicing.
- `scored.rs`: fulltext, hybrid, and vector top-k paths.
- `projected_read.rs`: projected scans, covering indexes, and pushdown reads.
- `plan_inspection.rs`: query and expression feature detection.

## SQL

Keep parser, binder, AST, and function metadata separate. When parser or binder files grow, split by statement family rather than by syntax token.

## Tests

Integration tests should be named for the subsystem under test. Prefer adding a new focused test file over extending an already-large file.

Use the file-size audit from `AGENTS.md` before starting broad feature work.
