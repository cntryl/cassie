# Phase 05 Issue 05: Bulk Rebuild Fast Paths

Milestone: Read-Model Write Optimization
Area: Rebuild
Status: Open
Priority: P2

## Requirements

Provide bulk-oriented rebuild and backfill write paths that exploit known replay/rebuild workflow shape instead of paying interactive per-row costs.
Rebuilds are bulk lifecycle workflows and should not be disguised as repeated interactive writes.

## Dependencies

- Depends on phase 05 issue 01 for rebuild write contracts.
- Depends on phase 05 issue 02 for batching foundations.
- Depends on phase 02 rebuild benchmark/verification work where rebuild state is measured.

## Handoff

- Provides the rebuild-optimized write path used by large projection backfills and version builds.

## Functional Scope

- Distinguish interactive projection mutation from rebuild/backfill modes explicitly in the write path.
- Build into inactive projection/version/index targets when the lifecycle requires isolation from active reads.
- Use bulk-friendly buffering, index-maintenance ordering, and metadata updates for rebuild workflows.
- Stream source rows/documents through bounded buffers instead of materializing complete rebuild input where possible.
- Use rebuild-local schema/index/projection metadata and avoid per-row parse/bind/plan work.
- Make rebuild flush boundaries, checkpoint updates, and verification metadata compatibility explicit.
- Preserve deterministic final state, checkpoint semantics, verification compatibility, and swap readiness.
- Expose rebuild write throughput, flush counts, and write amplification diagnostics.

## Required Write Path

- Explicit rebuild/backfill mode distinct from interactive mutation.
- Streaming source scan with bounded buffers.
- Bulk writes into inactive target/version/index namespaces where applicable.
- Metadata-only activation/swap where the data has already been built and verified.
- Rebuild metrics that separate source scan, target writes, index writes, verification-adjacent work, and activation metadata.

## Forbidden Write Path

- Running a full SQL parse/bind/plan cycle per rebuilt row.
- Writing partial rebuild state directly into the active read target unless the lifecycle explicitly allows it.
- Rebuilding indexes by repeatedly invoking interactive row mutation hooks without batching/coalescing.
- Rewriting projection data during version activation when a metadata swap is sufficient.
- Skipping verification-compatible metadata to gain rebuild speed.

## Implementation Plan

### Step 1: Add rebuild-mode tests

- Extend `tests/projection_lifecycle.rs` or split into `tests/projection_rebuild_fast_path.rs`.
- Add `should_build_projection_version_without_rewriting_active_rows`:
  - Arrange a materialized projection with an active version.
  - Act by building a new version.
  - Assert active reads remain stable until activation and activation does not change row data unexpectedly.
- Add `should_leave_failed_rebuild_target_retry_safe`:
  - Arrange a rebuild that fails through an injected or deterministic error path where available.
  - Assert metadata reports failed/cleanup-safe state and active projection remains readable.
- Add `should_record_rebuild_fast_path_metrics` after issue 06 counters exist.

### Step 2: Identify current rebuild flow

- Read `src/executor/execution/materialized_projection.rs` and document:
  - source query execution
  - target collection/version write path
  - refresh/build metadata updates
  - activation path
  - verification metadata interaction
- Identify any per-row use of SQL parse/bind/plan or interactive document mutation inside rebuild loops.

### Step 3: Add explicit rebuild write mode

- Introduce an internal enum such as `ProjectionWriteMode` with `Interactive`, `ReplayBatch`, `Rebuild`, and `IndexBackfill` variants if the same storage helper needs mode-specific behavior.
- Reuse the batch document helper from issue 02 for rebuild target writes.
- For rebuild mode:
  - load schema/index/projection metadata once
  - stream source rows in bounded batches
  - write target rows in batch chunks
  - update rebuild metadata at chunk or phase boundaries
  - defer expensive derived refreshes until safe batch boundaries

### Step 4: Preserve active/inactive isolation

- Ensure refresh/build into inactive version or rebuild target does not corrupt active reads.
- Keep activation as metadata-only when data is already materialized and verification policy allows activation.
- If current storage layout cannot separate active/inactive data cleanly, document that as a dependency on issue 04 rather than hiding it.

### Step 5: Verification compatibility

- Preserve row-hash/range/root metadata expectations when rebuilds write target data.
- Do not skip hash or verification-state updates unless the target is explicitly marked stale/unverified.
- Make EXPLAIN/metrics distinguish rebuild source scan, target write, verification, and activation.

### Step 6: Benchmark validation

- Keep `projection_refresh`, `projection_verify`, and `projection_swap` in `tier3_system_rebuild`.
- Add a separate benchmark only if the existing refresh benchmark cannot isolate target-write cost.
- Confirm `projection_swap` remains bounded with fixture size; if it scales with row count, treat that as a contract miss.

## Non-Goals

- Do not skip rebuild verification or lifecycle metadata updates.
- Do not reuse rebuild fast paths for interactive writes unless the contract says they are equivalent.

## Acceptance Criteria

- Rebuild workflows remain semantically identical and verifiable.
- Bulk rebuild benchmarks show lower elapsed time or reduced write amplification versus the generic path.
- Rebuild metrics distinguish bulk mode from interactive mode.
- Version activation remains bounded and does not rewrite rebuilt rows.
- Cancellation or timeout leaves rebuild targets in a failed, cleanup-safe, or retry-safe state.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering rebuild mode selection, deterministic output, metadata correctness, and cancellation cleanup.
- Include integration coverage for rebuild/versioned projection workflows touched by the fast path.

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
- `cargo test --locked --test projection_lifecycle --test integration_sql_projection --test midge_metadata_stats`
- `cargo test --locked --test metrics_runtime --test metrics_feedback`
- `cargo test --locked`
- `cargo bench --locked --bench tier3_system_rebuild --no-run`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
