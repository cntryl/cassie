# Phase 06 Issue 01: Read Access-Path Contracts

Milestone: Read-Model Read Optimization
Area: Contracts
Status: Open
Priority: P2

## Requirements

Define support for Cassie read patterns in terms of Midge-native access paths, not merely correct SQL results.
This issue operationalizes the access-path discipline described in `docs/performance-contracts.md`.

## Dependencies

- Depends on `docs/performance-contracts.md`.

## Handoff

- Provides the read-side contract set consumed by the rest of phase 06.

## Functional Scope

- Fill in contract targets for primary lookup, secondary lookup, range scan, ordered page, filtered page, count/exists, aggregates, full-text, vector, hybrid, time-bucket, column batch, and projection-shaped join-like reads.
- For each pattern, define required and forbidden access-path characteristics.
- Map each pattern to planner/explain assertions and deterministic benchmark ownership.

## Implementation Plan

### Step 1: Inventory current read paths

- Read `src/planner/physical.rs` and document existing plan signals: `predicate_pushdown`, `projected_scan_fields`, `scan_limit`, `selected_index`, `covered_index`, `column_batch_index`, `top_k`, `top_k_limit`, `join_strategy`, and aggregate acceleration flags.
- Read `src/app/query.rs` EXPLAIN generation and document which plan signals are already user-visible.
- Read `src/executor/executor.rs` dispatch order for vector top-K, full-text/scored paths, ordered column top-K, projected filtered reads, rollups, and fallback execution.
- Read `src/executor/execution/projected_read.rs`, `src/executor/scan.rs`, `src/executor/execution/scored.rs`, and `src/executor/execution/scored/vector_topk.rs` to identify where broad materialization still happens.
- Read `src/midge/adapter/documents.rs` and `src/midge/adapter/column_batches.rs` to document current scan capabilities, limits, filters, column batches, and missing bounded prefix/range APIs.

### Step 2: Define access-path vocabulary

- Add or refine terms in `docs/performance-contracts.md`: point lookup, index seek, covering index scan, projected row scan, bounded prefix scan, bounded range scan, column-batch scan, full-text candidate path, vector top-K path, hybrid candidate merge, rollup rewrite, projection-shaped read, and degraded fallback.
- Define forbidden paths for each pattern: full collection scan, late sort, broad materialization, offset discard scan, full-corpus rerank, runtime-heavy join, and hidden fallback.
- Mark each access path as implemented, planned, or unsupported.

### Step 3: Fill read contracts

- For each pattern in this issue, fill the existing contract sections in `docs/performance-contracts.md`.
- Keep initial latency numbers as measured placeholders unless benchmark data has been captured.
- For every supported pattern, include `Required access-path assertions` and `Forbidden plan shape`.
- For unsupported patterns, explicitly say whether users must materialize the projection shape.

### Step 4: Map validation ownership

- Planner shape tests: `tests/planner_physical.rs`, `tests/planner_indexes.rs`, `tests/planner_estimates.rs`.
- EXPLAIN tests: `tests/integration_sql_explain.rs`, plus subsystem-specific explain tests for scalar/vector/hybrid/column batches.
- Executor behavior tests: `tests/executor_query_sources.rs`, `tests/executor_sort.rs`, `tests/executor_limits.rs`, or focused subsystem files.
- Benchmarks: `tier2_subsystem_executor`, `tier2_subsystem_search`, `tier2_subsystem_vector`, `tier2_subsystem_hybrid`, `tier3_system_query`, and `tier3_system_query_breakdown`.

### Step 5: Close the contract issue

- Do not change planner/executor behavior here unless a tiny diagnostic helper is needed to make the contracts measurable.
- Update issue 06 references if new diagnostics are required to make an access path assertable.

## Non-Goals

- Do not change planner/executor implementation in this issue unless needed to make the contract measurable.
- Do not define support for patterns that Cassie cannot lower into an efficient path.

## Acceptance Criteria

- Each supported read pattern has an explicit access-path contract.
- Each contract names required and forbidden plan behavior.
- Each contract identifies benchmark and assertion ownership.
- Unsupported patterns are documented as non-goals or materialization requirements.

## Required Tests

- Add docs/benchmark support only where needed.
- If reusable fixture code is added, include deterministic fixture tests in `should_` style.

## Close-Out Steps

- Confirm every requirement and acceptance criterion above is implemented and documented.
- Keep source, test, and benchmark files under 1,000 lines; split focused modules/tests before adding large blocks.
- Update roadmap/docs references when the contract surface changes.
- Run the validation commands below in order.
- Run `cntryl-tools validate-tests -f <path>` for every touched test file.
- Delete this issue file only after implementation, validation, documentation, and close-out checks are complete.

## Validation

- `cargo build --locked`
- `cargo test --locked`
- `cargo bench --locked --bench tier2_subsystem_executor --no-run`
- `cargo bench --locked --bench tier2_subsystem_search --no-run`
- `cargo bench --locked --bench tier2_subsystem_vector --no-run`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
