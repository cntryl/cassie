# Phase 06 Issue 04: Projection-Shaped Read Layouts

Milestone: Read-Model Read Optimization
Area: Projections
Status: Open
Priority: P2

## Requirements

Require latency-sensitive read models to shape data, keys, and derived projections around expected reads when generic SQL lowering cannot reach an efficient path.

## Dependencies

- Depends on the archived phase 04 read access-path contract surface in `docs/performance-contracts.md` and `issues/phase-04/README.md` for read contracts.
- Depends on phase 01 projection lifecycle foundations for materialized/versioned projections.

## Handoff

- Provides the projection-design optimization path when planner/executor work alone is insufficient.

## Functional Scope

- Define when a read pattern must be satisfied by a materialized or derived projection instead of generic runtime joins/scans.
- Add planning/diagnostic hooks that make projection-shaped read paths explicit.
- Document the boundary between supported efficient runtime reads and required projection shaping.

## Required Access Path

- Latency-sensitive multi-entity or multi-shape reads use a declared projection/materialized shape.
- Planner/executor identifies that the source is a projection-shaped read.
- EXPLAIN reports the projection name, active output collection/version where applicable, freshness, and fallback status.
- Runtime-heavy joins remain correct but are marked as degraded for read-model contracts unless explicitly optimized.

## Forbidden Access Path

- Treating arbitrary runtime joins as satisfying projection-shaped read contracts.
- Hidden rewrites from base collections to projections without diagnostic visibility.
- Serving stale or inactive projection data as if it were fresh.
- Rebuilding projection data during a read to satisfy an interactive query.

## Implementation Plan

### Step 1: Document projection-shaped read contract

- Extend `docs/performance-contracts.md` with a `Projection-shaped read` pattern.
- State that product-critical join-like reads should usually query a materialized projection directly.
- Define when runtime joins are acceptable: small/admin/non-contract paths or explicitly benchmarked optimized joins.

### Step 2: Add tests around current materialized source behavior

- Extend `tests/projection_lifecycle.rs` or `tests/integration_sql_projection.rs`.
- Add `should_explain_materialized_projection_read_shape`:
  - Arrange a materialized projection with active output.
  - Act with `EXPLAIN SELECT ... FROM projection_name`.
  - Assert plan reports projection-shaped read, active output collection/version, and freshness.
- Add `should_mark_runtime_join_as_degraded_for_projection_shaped_contract` in `tests/integration_sql_join_plans.rs` or EXPLAIN tests.
- Add freshness/fallback assertions if existing projection metadata exposes freshness.

### Step 3: Add catalog/read-shape metadata only if needed

- Prefer using existing materialized projection metadata first.
- If explicit read-shape metadata is needed, add it under `src/catalog/` near materialized projection metadata rather than inventing a planner-only registry.
- Keep metadata Cassie-specific and visible through catalog/EXPLAIN.

### Step 4: Wire source execution diagnostics

- `src/executor/execution/source.rs` already resolves materialized projection names to active output collections.
- Add a small plan/explain helper that identifies when `physical.collection` or source maps to a materialized projection.
- Include active version/output collection and freshness in `src/app/query.rs` EXPLAIN.
- Keep reads using the active output collection; do not rebuild or refresh during query execution.

### Step 5: Define fallback/degraded behavior

- Runtime joins remain semantically correct.
- EXPLAIN should show `projection_shape=runtime_join_degraded` or equivalent for join-like reads that are not materialized and not otherwise optimized.
- Do not fail correct SQL queries merely because they are not contract-optimized unless a future strict mode is explicitly introduced.

### Step 6: Benchmark validation

- Add or update a `tier3_system_query` or `tier3_system_query_breakdown` case comparing projection-shaped read against runtime join only after diagnostics exist.
- Use the benchmark to document why projection shaping is required for product-critical read models.

## Non-Goals

- Do not claim a generic runtime optimization for every read-model query shape.
- Do not turn projection shaping into a hidden planner rewrite without visibility.

## Acceptance Criteria

- The doc and planning surface clearly distinguish runtime-efficient paths from projection-required paths.
- Latency-sensitive join-like or multi-shape reads can be declared projection-backed.
- EXPLAIN/diagnostics identify when a projection-shaped path is being used.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering projection-required path selection, runtime fallback, and explain/diagnostic visibility.
- Include integration coverage for projection-backed reads.

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
- `cargo test --locked --test projection_lifecycle --test integration_sql_projection --test integration_sql_joins --test integration_sql_join_plans`
- `cargo test --locked --test planner_indexes --test planner_physical --test integration_sql_explain`
- `cargo test --locked`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
