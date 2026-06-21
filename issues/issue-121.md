# Issue 121: Projection Swaps

Milestone: V4 - Analytical Overlay
Area: Materialization
Status: Open
Priority: P3

## Requirements

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

## Close-Out Steps

- Confirm every requirement and acceptance criterion above is implemented and covered by tests.
- Keep source, test, and benchmark files under 1,000 lines; split focused modules/tests before adding large blocks.
- Keep new code in the owning subsystem shown in `AGENTS.md` and `docs/module_organization.md`; do not introduce a second storage abstraction.
- Update docs/catalog/EXPLAIN/metrics references when user-visible behavior changes.
- Run the validation commands below in order, including `cargo build --locked` before tests.
- Run `cntryl-tools validate-tests -f <path>` for every touched test file.
- Delete this issue file only after implementation, validation, documentation, and close-out checks are complete.

## Validation

- `cargo build --locked`
- `cargo test --locked --test parser_cte_schema --test planner_commands --test planner_logical`
- `cargo test --locked --test integration_sql_catalog --test integration_sql_projection --test views`
- `cargo test --locked --test catalog_introspection --test midge_metadata_stats`
- `cargo test --locked`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
