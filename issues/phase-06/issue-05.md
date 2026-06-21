# Phase 06 Issue 05: Access-Path Assertions And Diagnostics

Milestone: Read-Model Read Optimization
Area: Observability
Status: Open
Priority: P2

## Requirements

Make supported read paths provable by plan-shape assertions, EXPLAIN output, and metrics so Cassie can detect when it has regressed into generic SQL execution over Midge.

## Dependencies

- Depends on phase 04 issue 07 for the contract definitions.
- Consumes planner/executor behavior from phase 06 issues 01 through 04.

## Handoff

- Provides the assertion and observability layer required to lock in read optimizations.

## Functional Scope

- Add plan/explain metadata that distinguishes prefix scan, bounded range scan, top-N path, keyset continuation, projection-shaped path, and degraded fallback.
- Add test helpers or focused assertions for supported access-path contracts.
- Expose enough runtime metrics to relate latency regressions back to path selection.

## Required Diagnostic Path

- Planner records the access path it can prove.
- Executor records the path it actually used.
- EXPLAIN shows optimized, degraded, and fallback paths honestly.
- Tests can assert access-path contracts without parsing fragile prose where practical.
- Benchmarks can black-box path diagnostics to prevent optimized paths from silently regressing.

## Forbidden Diagnostic Path

- EXPLAIN labels that imply Midge-native execution when the executor still uses a collection scan.
- Latency-only validation with no path assertion.
- Metrics that cannot distinguish optimized path from fallback.
- Access-path assertions that require inspecting private storage internals manually.

## Implementation Plan

### Step 1: Add physical diagnostic fields

- Extend `PhysicalPlan` in `src/planner/physical.rs` with explicit read diagnostics after phase 04 issue 07 and phase 06 issue 01 settle the vocabulary:
  - `access_path`
  - `access_path_reason`
  - `fallback_reason`
  - `pagination_strategy`
  - `top_k_mode`
  - `projection_shape`
- Use enums with serde support if feasible; otherwise use stable strings.
- Update `src/runtime/helpers.rs` sample plan and any tests that construct `PhysicalPlan`.

### Step 2: Add executor path result reporting

- Add a small crate-private struct such as `ReadPathReport` in `src/executor/scan.rs` or a focused module.
- Report rows scanned/emitted, batches read, storage path used, fallback reason, early-stop reason, and index/column-batch/projection identifiers where applicable.
- Thread reports through projected reads, column batches, scored paths, and materialized projection reads incrementally.
- Keep result semantics unchanged.

### Step 3: Extend runtime snapshots

- Add a read-access-path snapshot in `src/runtime/snapshots.rs`, or extend existing relevant snapshots if that is less invasive.
- Counters should cover:
  - collection scans
  - point lookups
  - index seeks
  - prefix scans
  - range scans
  - bounded scans
  - keyset scans
  - offset degraded scans
  - storage top-K
  - heap top-K
  - projection-shaped reads
  - fallback scans
- Add methods in `src/runtime/*` to record these events at executor ownership boundaries.

### Step 4: Improve EXPLAIN output

- Update `src/app/query.rs` to include stable labels:
  - `access_path=...`
  - `access_path_reason=...`
  - `fallback=...`
  - `pagination=...`
  - `top_k_mode=...`
  - `projection_shape=...`
  - `early_stop=...`
- Keep existing labels for compatibility with current tests.
- Avoid including bind values in diagnostics.

### Step 5: Add assertion helpers

- Add test helper functions in the relevant test support module, or local helpers in EXPLAIN tests:
  - `assert_explain_contains_access_path`
  - `assert_explain_contains_fallback`
  - `assert_explain_not_optimized`
- Prefer simple string assertions while EXPLAIN remains textual, but centralize repeated checks to reduce brittle tests.

### Step 6: Add tests

- Extend `tests/integration_sql_explain.rs` for common labels.
- Extend `tests/planner_physical.rs` for physical diagnostic fields.
- Extend `tests/metrics_runtime.rs` or a focused metrics file for runtime counters.
- Add subsystem-specific tests only when a path belongs to that subsystem, such as column batches, vector, hybrid, or rollups.

### Step 7: Benchmark hooks

- Update benchmark workloads to black-box path diagnostics for:
  - executor ordered page
  - search top-K
  - vector top-K
  - hybrid top-K
  - query breakdown
- Do not print diagnostics from benchmarks.

## Non-Goals

- Do not overfit diagnostics to one benchmark fixture.
- Do not expose misleading plan labels when proof is absent.

## Acceptance Criteria

- Supported read patterns have plan-shape or EXPLAIN assertions.
- Benchmarks and tests can tell whether the intended access path was selected.
- Diagnostics are sufficient to explain path regressions without inspecting storage internals manually.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering plan labels, fallback labels, metrics updates, and assertion helpers.
- Include planner, integration, and metrics coverage.

## Close-Out Steps

- Confirm every requirement and acceptance criterion above is implemented and covered by tests.
- Keep source, test, and benchmark files under 1,000 lines; split focused modules/tests before adding large blocks.
- Keep new code in the owning subsystem shown in `AGENTS.md` and `docs/module-organization.md`; do not introduce a second storage abstraction.
- Update docs/catalog/EXPLAIN/metrics references when user-visible behavior changes.
- Run the validation commands below in order, including `cargo build --locked` before tests.
- Run `cntryl-tools validate-tests -f <path>` for every touched test file.
- Delete this issue file only after implementation, validation, documentation, and close-out checks are complete.

## Validation

- `cargo build --locked`
- `cargo test --locked --test integration_sql_explain --test planner_indexes --test planner_physical`
- `cargo test --locked --test metrics_runtime --test metrics_search --test metrics_feedback`
- `cargo test --locked`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
