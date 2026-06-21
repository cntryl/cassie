# Phase 07 Issue 04: Vectorized Joins

Milestone: Advanced Backlog
Area: Execution
Status: Open
Priority: P3

## Requirements

Execute eligible join build/probe operations in batches to reduce per-row overhead while preserving SQL join semantics.
This issue accelerates existing join semantics; it must not introduce a second set of result rules for batched execution.

## Dependencies

- Depends on phase 03 issue 06 for column-native execution paths.
- Depends on phase 03 issue 07 for hybrid row/column planning.
- Depends on phase 03 issue 08 for parallel execution foundations.
- Depends on phase 03 issue 09 for vectorized execution conventions and batch memory accounting.
- Depends on phase 04 issue 06 for runtime-boundary regression rules.
- Depends on phase 06 issue 05 for access-path/executor diagnostics.
- Can use phase 07 issue 02 column-store tables when present, but must also work with existing batch/row inputs.

## Handoff

- Provides a vectorized join alternative that phase 07 issue 05 adaptive execution plans and phase 07 issue 06 runtime operator switching can pre-validate.

## Functional Scope

- Add vectorized/batch kernels for equi-join key extraction, hash build/probe, match materialization, and null-key handling.
- Support inner and left joins first, with right/full/semi/anti support only when semantics are explicitly implemented and tested.
- Use batch/column inputs where available and materialize rows only for matched output or unsupported downstream operators.
- Preserve duplicate-key behavior, null semantics, projection aliases, SQL-visible ordering guarantees, deterministic internal tie behavior, timeout/cancellation, and memory/spill budgets.
- Define row-to-batch and batch-to-row boundaries so unsupported downstream operators fall back without losing type/null information.
- Keep batch sizes configurable and bounded by the existing query memory budget.
- Preserve phase 04 cancellation and blocking-boundary expectations when batch work is entered from pgwire or REST.
- Report vectorized join selection, batch sizes, build/probe rows, matches, spills, and fallback through EXPLAIN/metrics.

## Non-Goals

- Do not change parser/binder join semantics.
- Do not implement non-equi vectorized joins in this issue.
- Do not require column-store tables for vectorized joins.

## Implementation Plan

### Step 1: Define vectorized join scope

- Define allowed join families, key shapes, and required batch ownership for the first implementation wave.
- Define safe fallback boundaries for unsupported key shapes and downstream operators.
- Define memory budget and spill behavior for batch buffers.

### Step 2: Add batch conversion and row materialization helpers

- Add utility methods to convert rows to compact batch keys and materialize output rows only when required.
- Preserve null/type/sparse behavior and alias semantics across conversions.
- Validate cancellation checks between conversion and materialization phases.

### Step 3: Implement vectorized build/probe kernels

- Implement batched hash/build and probe for inner and left join variants first.
- Handle duplicate keys and null-key behavior exactly as scalar execution baseline.
- Keep scalar path as fallback when batch transfer costs exceed configured thresholds.

### Step 4: Planner and executor wiring

- Extend plan nodes to represent vectorized hash build/probe selection with explicit guard conditions.
- Add executor mode-switch within non-adaptive execution for single-query path.
- Preserve timeout/shutdown behavior by draining/disposing batch state safely.

### Step 5: Observability and controls

- Add decision labels (enabled/disabled, fallback reason, batch size, spill count).
- Add metrics for build rows, probe rows, matched rows, and spill fallback events.
- Add config/feature gate for controlled rollout.

### Step 6: Validation and close-out

- Add planner/executor tests for supported join shapes, cancellation, spill limits, conversion correctness, and fallback paths.
- Add deterministic regression fixtures for semantic equivalence with scalar/hash join behavior.
- Add optional benchmark check that demonstrates reduced per-row overhead under supported workloads.

## Acceptance Criteria

- Vectorized join results match scalar/hash join results for supported join types and key shapes.
- Unsupported join types or predicates fall back deterministically.
- Memory/spill limits are enforced during batch build/probe.
- Benchmarks or metrics show reduced per-row overhead for eligible joins.
- Cancellation or timeout releases batch buffers and spill state.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering inner/left joins, duplicate keys, null keys, unmatched rows, row/batch conversion, fallback, spill/limit behavior, cancellation cleanup, and EXPLAIN diagnostics.
- Include planner and executor tests.

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
- `cargo test --locked --test planner_physical --test planner_logical --test planner_aggregates_sets`
- `cargo test --locked --test executor_parallel --test executor_query_sources --test executor_sort`
- `cargo test --locked --test integration_sql_joins --test integration_sql_join_plans --test integration_sql_aggregates`
- `cargo test --locked`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
