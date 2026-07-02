# Module Organization

Cassie keeps module boundaries small so feature work can be reviewed and tested surgically.

For a diagrammed map of the current module, execution, storage, runtime, and operational surfaces, see [Architecture Diagrams](architecture-diagrams.md).

## Targets

- Source files should stay under 1,000 lines.
- Test files should stay under 1,000 lines and cover one behavior family.
- Legacy files over 1,000 lines need a written reason and a concrete split plan before related feature work grows them.
- Tests should live in subsystem-specific integration files with shared helpers under `tests/support/`.

## Executor

`src/executor/mod.rs` is the public facade. Keep public exports there and keep execution internals under `src/executor/execution/`:

- `execution/mod.rs`: execution-internal facade and compatibility reexports for helper modules.
- `execution/entrypoints.rs`: public run APIs, session-aware execution entrypoints, and execution-breakdown assembly.
- `execution/dispatch.rs`: logical-plan dispatch and ordered access-path registry.
- `execution/cte.rs`: CTE context, recursive CTE execution, and CTE row bookkeeping.
- `execution/result.rs`: SELECT result assembly, row signatures, value comparison, and result helper utilities.
- `execution/dml_command.rs`: logical command dispatch, DML command orchestration, routine command execution, and write side effects.
- `execution/schema_command.rs`: schema, table, view, role, graph, and index DDL execution.
- `execution/dml.rs`: insert, update, and delete row mutations.
- `execution/source.rs`: sources, joins, set operations, distinct, and slicing.
- `execution/scored.rs`: full-text, hybrid, and vector top-k paths.
- `execution/projected_read.rs`, `execution/index_read.rs`, `execution/ordered_read.rs`, and `execution/time_series_read.rs`: access-path implementations.
- `execution/aggregate_exec.rs` and `execution/window_exec.rs`: aggregate and window execution.
- `execution/plan_inspection.rs`: query and expression feature detection.

Access-path selection must stay open/closed: add a focused executor function and register it in the ordered registry instead of growing inline chains in entrypoints.

## App

`src/app/mod.rs` is the application facade. It owns reexports and module wiring only; state, session behavior, error mapping, plan/cache provenance, and catalog hydration live in focused modules:

- `app/state.rs`: `Cassie` and runtime config state.
- `app/session.rs`: `CassieSession` and transaction staging.
- `app/error.rs`: `CassieError`, unsupported SQL mapping, and conversion impls.
- `app/cache.rs`: cache keys, normalized vector cache entries, plan-cache provenance, and app-local time helpers.
- `app/hydration.rs`: catalog, role, cardinality, and runtime-feedback hydration.

Lifecycle, query, document, vector, role, diagnostics, replay, snapshot, and operational methods stay in their existing service modules and extend `Cassie` through focused impl blocks.

## Midge Adapter

`src/midge/adapter/mod.rs` is the concrete Midge adapter facade. Midge remains the only storage layer; do not introduce a backend trait or alternate storage abstraction.

- `adapter/core.rs`: `Midge` construction, family bootstrap, storage layout readiness, and layout compatibility checks.
- `adapter/transactions.rs`: family transaction helpers and column-family lookup.
- `adapter/raw_ops.rs`: raw get and scan operations used by diagnostics, runtime caches, and capacity inspection.
- Existing domain modules under `adapter/` own schema, documents, metadata, indexes, projections, repair, verification, operational records, and capacity behavior.

Persisted keys must continue to flow through `adapter/key_encoding.rs`; storage format changes require an explicit migration plan before implementation.

## SQL

Keep parser, binder, AST, and function metadata separate. When parser or binder files grow, split by statement family rather than by syntax token.

Current oversized legacy source files that need dedicated extraction passes include `src/sql/parser.rs`, `src/sql/binder.rs`, `src/pgwire/connection.rs`, `src/executor/filter.rs`, `src/runtime.rs`, `src/executor/execution/scored.rs`, `src/midge/row_blob.rs`, `src/catalog/metadata.rs`, and `src/planner/physical.rs`.

Storage-adjacent acceleration such as column batches belongs in focused `src/midge/adapter/*` modules and must keep Midge as the direct storage layer. Do not add another storage abstraction for analytical overlays.

Projection verification and consistency workflows should stay split by ownership: app-level manifest/export comparison logic in `src/app/consistency.rs`, serializable manifest/report metadata in `src/catalog/consistency.rs`, catalog view rows in `src/catalog/virtual_views_consistency.rs`, and Midge persistence beside the existing metadata adapter methods.

## Tests

Integration tests should be named for the subsystem under test. Prefer adding a new focused test file over extending an already-large file.

Current oversized legacy test files that need dedicated split passes include `tests/parser.rs`, `tests/metrics.rs`, `tests/planner.rs`, `tests/midge_cf_layout.rs`, and `tests/pgwire_extended_query.rs`.

Use the file-size audit from `AGENTS.md` before starting broad feature work.
