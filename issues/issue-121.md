# Issue 121: Projection Swaps

Milestone: V4 - Analytical Overlay
Area: Materialization
Status: Open
Priority: P3

## Requirement

Atomically promote a built materialized projection version to active while preserving rollback and cleanup behavior.

## Functional Scope

- Add a SQL/admin path to swap the active projection version after the target version is built and verified.
- Perform the active-version pointer update atomically in catalog metadata so readers see either the old version or the new version, never a mixed version.
- Keep the previous active version retained as retired/rollback-capable until explicitly dropped or retention cleanup runs.
- Invalidate plan/result caches that depend on the swapped projection.
- Emit metrics and catalog diagnostics for swap success, failure, rollback, and active version.

## Non-Goals

- Do not rebuild projections in this issue; swaps operate on already-built versions.
- Do not implement distributed consensus for swaps across multiple Cassie instances.

## Acceptance Criteria

- Successful swap changes future reads to the new version without affecting in-flight query correctness.
- Failed swap leaves the previous active version intact and readable.
- Cache invalidation prevents stale plans/results from using the wrong version.
- Restart after swap hydrates the new active version.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering successful swap, invalid target version rejection, failure rollback, cache invalidation, restart hydration, and retired-version cleanup.
- Include integration and catalog tests.

## Closeout Steps

- Run the validation commands below.
- Run `cargo build --locked`.
- Run `cargo fmt --all -- --check`.
- Document swap and rollback operational semantics.

## Validation

- `cargo test --test integration_sql --quiet`
- `cargo test --test catalog_introspection --quiet`
- `cntryl-tools validate-tests -f tests/integration_sql.rs`
