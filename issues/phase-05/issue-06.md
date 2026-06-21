# Phase 05 Issue 06: Write Amplification Diagnostics

Milestone: Read-Model Write Optimization
Area: Diagnostics
Status: Open
Priority: P2

## Requirements

Make Cassie's write amplification visible enough that write optimization work can be explained in terms of row writes, index writes, metadata writes, and rebuild overhead.
Without these diagnostics, phase 05 cannot distinguish a real Midge-efficient path from a fast local fixture.

## Dependencies

- Depends on phase 05 issue 01 for the contract and benchmark framing.
- Consumes metrics from phase 05 issues 02 through 05 as they land.
- Consumes read-shape labels imported from phase 04 issue 07 by phase 05 issue 01.

## Handoff

- Provides the diagnostic baseline that future write optimization work must report against.

## Functional Scope

- Add metrics and diagnostics for row writes, index writes, metadata writes, batch flushes, rebuild writes, and failed/retried write work where applicable.
- Track replay events received, applied events, skipped duplicates, row puts/deletes, index puts/deletes, metadata puts/deletes, checkpoint updates, batch flushes, rebuild target writes, and activation metadata writes where those events are observable.
- Define derived write amplification ratios such as storage writes per applied event and index writes per row mutation.
- Expose the counters through existing runtime/admin surfaces.
- Document how write amplification is interpreted in phase 05 benchmarks.

## Required Diagnostic Path

- Counters are updated at the same ownership boundary that performs the write.
- Benchmarks can capture row/index/metadata/rebuild categories separately.
- Diagnostics identify whether the path was interactive, replay batch, duplicate skip, rebuild, index rebuild, or activation.
- Missing low-level Midge internals are represented as unavailable rather than invented.

## Forbidden Diagnostic Path

- Reporting only total latency with no write-category breakdown.
- Counting logical rows as storage writes when they are not equivalent.
- Hiding duplicate replay skips inside successful write counts.
- Requiring query-path access to low-level storage internals that Midge does not expose.

## Implementation Plan

### Step 1: Add snapshot fields first

- Extend `ProjectionSnapshot` in `src/runtime/snapshots.rs` with write amplification counters:
  - `write_row_puts`
  - `write_row_deletes`
  - `write_index_puts`
  - `write_index_deletes`
  - `write_metadata_puts`
  - `write_metadata_deletes`
  - `write_duplicate_checks`
  - `write_batch_flushes`
  - `write_rebuild_target_puts`
  - `write_activation_metadata_writes`
- Keep field names stable and JSON-friendly because metrics are user-visible.
- Default all counters to zero through the existing `Default` derive.

### Step 2: Add runtime recording APIs

- Add methods in `src/runtime/projection_metrics.rs`:
  - `record_projection_write_batch(projection, stats)`
  - `record_projection_index_writes(projection, puts, deletes)`
  - `record_projection_metadata_writes(projection, puts, deletes)`
  - `record_projection_rebuild_writes(projection, target_puts, flushes)`
  - `record_projection_activation_write(projection)`
- Use a small crate-private stats struct if it keeps call sites readable.
- Do not make runtime metrics depend on Midge transaction internals.

### Step 3: Wire counters at ownership boundaries

- Replay counters belong in `src/app/replay.rs` after a replay batch succeeds or fails with known skipped duplicate counts.
- Row/index write counters belong in Midge document/batch helpers because that code owns row/vector/index writes.
- Projection metadata counters belong in `src/midge/adapter/projections.rs` or the app layer if only the app knows semantic operation type.
- Rebuild counters belong in materialized projection execution after target writes are known.
- Activation counters belong in the version swap path, not in row-write helpers.

### Step 4: Add metrics tests

- Extend `tests/metrics_runtime.rs` with focused `should_` tests:
  - `should_record_projection_replay_write_amplification`
  - `should_record_duplicate_replay_checks_without_row_puts`
  - `should_record_projection_rebuild_write_categories`
  - `should_record_projection_activation_metadata_write`
- If tests need full replay setup, use `tests/projection_lifecycle.rs` for behavior and keep `metrics_runtime.rs` for runtime method/unit-level assertions.

### Step 5: Add benchmark reporting hooks

- Update `benches/support/workloads/system.rs` to black-box the relevant metrics after:
  - `projection_duplicate_replay`
  - `projection_lag_catchup`
  - `projection_refresh_workflow`
  - `projection_version_swap`
  - `index_rebuild_ddl`
- Do not print from benchmarks; consume metrics through return values or `black_box` so criterion output stays clean.

### Step 6: Document exactness

- Update `docs/performance-contracts.md` with which counters are exact and which are semantic approximations.
- Mark low-level Midge internals unavailable where the storage engine does not expose them.
- Define how to interpret a counter regression in phase 05 close-out.

## Non-Goals

- Do not build a second observability stack.
- Do not require exact low-level storage-engine internals that Midge does not expose.

## Acceptance Criteria

- Benchmarks can distinguish major write-cost categories.
- Metrics survive restart/hydration where the existing runtime model expects persistence.
- Diagnostics are sufficient to explain measured regressions or improvements in phase 05.
- Duplicate replay, replay catch-up, projection refresh, projection swap, and index rebuild benchmarks expose enough counters to explain their dominant write cost.
- Documentation explains which counters are exact and which are best-effort because Midge does not expose lower-level details.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering counter updates, restart/hydration where applicable, and benchmark-visible reporting hooks.
- Include runtime metrics tests.

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
- `cargo test --locked --test metrics_runtime --test metrics_feedback --test metrics_plan_pgwire`
- `cargo test --locked --test midge_metadata_stats --test operational_smoke`
- `cargo test --locked`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
