# Phase 01 Issue 05: Projection Swaps

Milestone: Read-Model Core
Area: Projection Lifecycle
Status: Open
Priority: P0

## Requirements

Atomically promote a built materialized projection version to active while preserving rollback and cleanup behavior.

## Dependencies

- Depends on phase 01 issue 04 for versioned projection metadata and active-version routing.
- Consumes phase 02 verification metadata when present, but must remain useful before Merkle verification is implemented.

## Handoff

- Completes the phase 01 projection lifecycle path: checkpointed source state, replay-safe ingestion, materialized projection definitions, versioned builds, and active-version swaps.
- Provides swap state and active-version metadata consumed by phase 02 rebuild verification and projection integrity verification.

## Functional Scope

- Add a SQL/admin path to swap the active projection version after the target version is built and eligible.
- Perform the active-version pointer update atomically in catalog metadata so readers see either the old version or the new version, never a mixed version.
- Keep the previous active version retained as retired/rollback-capable until explicitly dropped or retention cleanup runs.
- Invalidate plan/result caches that depend on the swapped projection.
- Emit metrics and catalog diagnostics for swap success, failure, rollback, and active version.
- If verification metadata exists, block swaps for failed or stale verification unless an explicit unsafe override exists and is tested.

## Non-Goals

- Do not rebuild projections in this issue; swaps operate on already-built versions.
- Do not implement distributed consensus for swaps across multiple Cassie instances.
- Do not implement Merkle verification itself; phase 02 issues provide row hashes, roots, and rebuild verification.

## Acceptance Criteria

- Successful swap changes future reads to the new version without affecting in-flight query correctness.
- Failed swap leaves the previous active version intact and readable.
- Cache invalidation prevents stale plans/results from using the wrong version.
- Restart after swap hydrates the new active version.
- Unsafe override, if implemented, is explicit, audited in diagnostics, and never the default.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering successful swap, invalid target version rejection, failure rollback, cache invalidation, restart hydration, and retired-version cleanup.
- Include integration and catalog tests.
- Include tests for verification-aware swap blocking when verification metadata is available and normal built-version swap when it is not.

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
- `cargo test --locked --test parser_cte_schema --test planner_commands --test planner_logical`
- `cargo test --locked --test integration_sql_catalog --test integration_sql_projection --test views`
- `cargo test --locked --test catalog_introspection --test midge_metadata_stats`
- `cargo test --locked`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
