# Phase 05 Issue 02: Replay And Ingest Batching

Milestone: Read-Model Write Optimization
Area: Write Path
Status: Open
Priority: P2

## Requirements

Batch replay and ingest work so Cassie preserves Midge locality and reduces per-row overhead across projection writes.
The batch must be a real write-path concept, not just a loop around single-row writes.

## Dependencies

- Depends on phase 05 issue 01 for write-side contracts.
- Depends on phase 01 replay/materialization lifecycle foundations already implemented.

## Handoff

- Provides the batched write-path foundations used by later phase 05 index and rebuild optimization work.

## Functional Scope

- Define batching boundaries for replay ingestion, SQL/REST bulk mutation, and rebuild-driven writes.
- Load collection schema, index metadata, replay metadata, and write options once per batch where possible.
- Validate event order, duplicate ids, checkpoints, and projection identity before durable writes where the existing replay contract allows it.
- Reduce repeated catalog/schema/index lookups across rows in the same batch.
- Reuse write-side buffers and encoding state safely across batches.
- Group row, index, and checkpoint writes by projection and locality-friendly key prefix where possible.
- Preserve deterministic ordering, timeout/cancellation behavior, and idempotent replay semantics.
- Surface batch size, batch flush count, and replay/write latency metrics.

## Required Write Path

- One batch-level replay context per projection/source/batch id.
- Batch-local schema/index/replay metadata lookup.
- Idempotency checks before row/index rewrites for duplicate events.
- Row writes and index writes grouped by projection and flush boundary.
- Checkpoint/lag metadata updated once per successful batch or documented sub-batch boundary.

## Forbidden Write Path

- Calling the full single-row SQL planning path for each replay event.
- Re-reading catalog/index metadata for every row in the same batch.
- Rewriting row or index data for duplicate replay events.
- Updating projection checkpoint metadata after every event when a batch-level update is sufficient.
- Materializing the whole batch into broad intermediate state when streaming batch chunks would satisfy the contract.

## Implementation Plan

### Step 1: Write failing replay batching tests

- Extend `tests/projection_lifecycle.rs` or create `tests/projection_replay_batching.rs` if the existing file is near the size limit.
- Add `should_apply_replay_batch_with_single_checkpoint_update`:
  - Arrange a projection table and a batch with multiple ordered events.
  - Act by calling `Cassie::replay_projection_batch`.
  - Assert rows are visible, checkpoint reflects the last event, applied count equals the batch length, and replay metrics record one batch.
- Add `should_skip_duplicate_replay_without_rewriting_document`:
  - Arrange an event, apply it once, then apply the same event with a different payload.
  - Assert the stored row keeps the original payload, duplicate count increments, and applied count does not.
- Add `should_reject_out_of_order_batch_before_partial_batch_writes`:
  - Arrange current source position and a batch containing an invalid older event.
  - Assert the error is reported and no later event from the failed batch is written.
- Add timeout/cancellation cleanup coverage only after a runtime control exists in the touched path.

### Step 2: Introduce internal batch write types

- Add a small internal type near replay ownership, likely in `src/app/replay.rs` or a new `src/app/replay_batch.rs` if the file begins to grow:
  - `ReplayBatchContext`: projection, source identity, batch id, starting metadata.
  - `ReplayWriteOp`: document id plus put/delete payload.
  - `ReplayBatchPlan`: ordered applied ops, skipped duplicate ids, final checkpoint metadata.
  - `ReplayBatchStats`: applied, skipped, duplicate checks, row puts, row deletes, event records, metadata writes.
- Keep these types internal unless tests need public inspection through metrics.

### Step 3: Split replay preflight from durable writes

- Refactor `Cassie::replay_projection_batch` into clear phases:
  - validate projection/source/batch fields
  - load projection metadata once
  - scan events once for empty ids and monotonic positions
  - check duplicate event ledger entries
  - build a replay plan for only non-duplicate events
  - apply durable writes
  - persist final replay metadata once
  - record runtime metrics
- If duplicate ledger checks remain one key lookup per event, keep that explicit and count it; do not hide it as a row rewrite.

### Step 4: Add a Midge batch document API

- Add a focused storage helper, preferably `src/midge/adapter/document_batches.rs`, and wire it from `src/midge/adapter.rs`.
- Define `DocumentWriteOp` and `DocumentWriteBatchReport` internally to Midge or crate-private.
- Implement `Midge::apply_document_write_batch(collection, ops)`:
  - load collection schema once
  - load row schema once
  - load vector index metadata once
  - encode all put payloads using the shared row schema
  - open one data write transaction for row/blob/vector/hash changes where Midge transaction boundaries allow it
  - delete normalized vector keys only for documents that are actually changed or deleted
  - rebuild column batches once after the batch when needed
  - refresh projection hashes once with aggregate row delta where correctness permits
- Keep `put_document` and `delete_document` behavior intact by routing single-row calls through the batch helper only after tests prove equivalence.

### Step 5: Batch projection event ledger writes

- Add `Midge::record_projection_events_batch` in `src/midge/adapter/projections.rs`.
- Use one schema transaction for all non-duplicate event ledger records in a batch.
- Keep `record_projection_event` as a single-event wrapper if existing callers need it.

### Step 6: Update metrics and benchmarks

- Extend runtime projection metrics only as far as needed for this issue; full write amplification belongs to issue 06.
- Update `benches/support/workloads/system.rs` if needed so `projection_lag_catchup` exercises the new batch path with 64 events.
- Keep `projection_duplicate_replay` focused on duplicate skip behavior.

### Step 7: Validate equivalence

- Run targeted replay/materialized tests first, then full test/bench compile validation from this issue.
- Confirm no source, test, or benchmark file crosses 1,000 lines; split before adding broad helper code.

## Non-Goals

- Do not weaken correctness or replay determinism for higher throughput.
- Do not add a second storage abstraction or external queue.

## Acceptance Criteria

- Batched ingestion produces identical logical projection state to unbatched ingestion.
- Replay/ingest benchmarks show reduced per-row overhead for eligible workloads.
- Cancellation and timeout release batch-local resources cleanly.
- Metrics and diagnostics expose when batching is active.
- Duplicate replay paths skip row/index rewrites and expose skipped duplicate counts.
- Benchmarks or counters can distinguish per-event work from per-batch work.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering deterministic batch application, duplicate replay handling, timeout/cancellation cleanup, and metric exposure.
- Include integration coverage for replay or bulk ingest paths touched by the batching implementation.

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
- `cargo test --locked --test integration_sql_insert_values --test integration_sql_update --test integration_sql_delete`
- `cargo test --locked --test plan_cache --test operational_smoke --test midge_metadata_stats`
- `cargo test --locked`
- `cargo bench --locked --bench tier2_subsystem_ingest --no-run`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
