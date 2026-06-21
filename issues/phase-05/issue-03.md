# Phase 05 Issue 03: Index Maintenance Batching

Milestone: Read-Model Write Optimization
Area: Indexes
Status: Open
Priority: P2

## Requirements

Reduce write amplification from secondary index maintenance by coalescing and ordering index updates around the actual read-model write shapes.
Index maintenance must be a delta-oriented write path, not a hidden rebuild or per-row generic rewrite.

## Dependencies

- Depends on phase 05 issue 01 for write contracts.
- Depends on phase 05 issue 02 for write batching foundations.

## Handoff

- Provides lower-overhead index maintenance for replay, ingest, and rebuild workloads.

## Functional Scope

- Batch index entry creation, update, and delete work for scalar, composite, covering, search, and vector metadata where safe.
- Avoid duplicate index maintenance within a batch when intermediate states are superseded before flush.
- Compute old/new indexed field deltas once per logical row mutation.
- Skip index writes for unchanged indexed values.
- Group index writes by collection/index/key prefix where possible.
- Keep uniqueness checks deterministic before visible writes.
- Preserve uniqueness, constraint, and visibility semantics.
- Track index-maintenance writes separately from base row writes in diagnostics and benchmarks.

## Required Write Path

- Delta-based index maintenance for insert, update, delete, replay, and rebuild paths.
- Batch coalescing when the same row is updated multiple times before flush and only the final visible index state matters.
- Ordered or grouped index writes for locality where the index format supports it.
- Separate counters for row writes and index writes.

## Forbidden Write Path

- Full index rebuild for ordinary row mutation.
- Delete-and-reinsert of unchanged index entries.
- Per-index catalog rediscovery for every row in a batch.
- Deferring correctness-critical unique or constraint checks beyond the visible write boundary.
- Treating search/vector/covering indexes as invisible side effects with no write amplification accounting.

## Implementation Plan

### Step 1: Add failing index-delta tests

- Add focused tests to the indexed mutation area, splitting to a new file if existing index tests are too large.
- Add `should_not_rewrite_unchanged_vector_index_entry_on_non_vector_update`:
  - Arrange a vector-indexed collection and insert a row.
  - Act by updating only a non-indexed field through SQL or replay.
  - Assert vector index lookup still returns the row and write amplification counters do not report a vector rewrite once issue 06 counters exist.
- Add `should_update_only_changed_scalar_index_entries`:
  - Arrange scalar/composite indexes.
  - Act by updating one indexed field.
  - Assert old index lookup no longer matches and new lookup does, without requiring a full index rebuild.
- Add `should_preserve_unique_constraint_during_batched_index_maintenance`:
  - Arrange unique index values in a batch.
  - Assert duplicate detection occurs before visible writes.

### Step 2: Define index delta model

- Add a small internal model in the owning storage/index module:
  - `IndexedFieldDelta`: index name, old key/value membership, new key/value membership.
  - `IndexWriteOp`: insert/delete/update for scalar, composite, covering, full-text, vector-normalized, and column-batch metadata where supported.
  - `IndexMaintenanceReport`: puts, deletes, unchanged_skips, uniqueness_checks.
- Keep the first implementation scoped to indexes that currently have physical write behavior. Document no-op metadata-only indexes explicitly.

### Step 3: Reuse row encode/decode work

- Extend the batch document helper from issue 02 to optionally load the old row once when an update may affect indexed fields.
- Compute old/new index membership once per logical row mutation.
- Skip index maintenance for indexes whose membership did not change.
- Keep vector normalized record generation in `src/midge/adapter/metadata.rs`, but add a path that compares existing vector membership before deleting/reinserting.

### Step 4: Preserve correctness boundaries

- Perform unique/constraint validation before committing visible row/index writes.
- Keep old `put_document` and `delete_document` APIs behavior-compatible.
- For delete paths, delete only index entries belonging to the deleted document, not broader collection/index prefixes.

### Step 5: Add diagnostics hooks

- Add crate-private report values from index maintenance so issue 06 can record row/index write amplification without inspecting storage internals.
- Do not expose incomplete counters as public metrics until issue 06 wires the snapshot.

### Step 6: Benchmark validation

- Extend `tier2_subsystem_ingest` with an indexed update/replay case only if the current `projection_write_path` cannot distinguish index-maintained writes.
- Keep `index_rebuild_ddl` in `tier3_system_rebuild` as the rebuild/backfill benchmark, not the ordinary mutation benchmark.

## Non-Goals

- Do not defer correctness-critical index maintenance beyond the durability/visibility model Cassie already exposes.
- Do not make index updates eventually consistent for normal query paths.

## Acceptance Criteria

- Index-maintained writes remain semantically identical to existing behavior.
- Eligible workloads show lower write amplification or lower per-row write cost.
- Unique/constraint behavior remains deterministic under batched maintenance.
- Metrics distinguish row writes from index writes.
- Tests prove unchanged indexed fields do not trigger unnecessary index rewrites where the implementation exposes that behavior.
- Rebuild/index-DDL benchmarks can distinguish source scan cost from index write cost.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering insert/update/delete batching, duplicate supersession within a batch, uniqueness preservation, and diagnostic accounting.
- Include integration coverage for indexed SQL mutation paths.

## Close-Out Steps

- Confirm every requirement and acceptance criterion above is implemented and covered by tests.
- Keep source, test, and benchmark files under 1,000 lines; split focused modules/tests before adding large blocks.
- Keep new code in the owning subsystem shown in `AGENTS.md` and `docs/module-organization.md`; do not introduce a second storage abstraction.
- Update docs/catalog/metrics references when user-visible behavior changes.
- Run the validation commands below in order, including `cargo build --locked` before tests.
- Run `cntryl-tools validate-tests -f <path>` for every touched test file.
- Delete this issue file only after implementation, validation, documentation, and close-out checks are complete.

## Validation

- `cargo build --locked`
- `cargo test --locked --test integration_sql_scalar_indexes --test integration_sql_vector_indexes --test integration_sql_constraints`
- `cargo test --locked --test planner_indexes --test vector_index_metadata --test metrics_runtime`
- `cargo test --locked`
- `cargo bench --locked --bench tier2_subsystem_ingest --no-run`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
