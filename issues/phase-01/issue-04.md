# Phase 01 Issue 04: Projection Versioning

Milestone: Read-Model Core
Area: Projection Lifecycle
Status: Open
Priority: P0

## Requirements

Version materialized projection definitions and storage so new projection builds can coexist with the currently active version.

## Dependencies

- Depends on phase 01 issue 03 for materialized projection definitions and build output.

## Handoff

- Provides active/building/failed/retired version state consumed by phase 01 issue 05 swaps.

## Functional Scope

- Store projection versions with definition fingerprint, source schema epochs, output schema, storage prefix, build state, created timestamp, and active/retired markers.
- Route reads to exactly one active version unless an explicit admin/debug path requests another version.
- Allow a new version to build from source rows without corrupting or replacing the active version.
- Keep versioned index, column, and metadata entries isolated by projection version.
- Expose active, building, failed, and retired versions through catalog/introspection and metrics.
- Keep projection checkpoint/freshness metadata version-aware where it describes versioned output.

## Non-Goals

- Do not implement atomic active-version swaps here; that is phase 01 issue 05.
- Do not support concurrent writes directly into projection output versions.
- Do not implement Merkle/hash verification here; phase 02 verification issues add stronger safety gates.

## Acceptance Criteria

- Multiple projection versions can exist without key collisions or mixed reads.
- Restart hydration preserves version state and active-version routing.
- Failed builds leave the previous active version readable.
- Dropping a version cleans up its storage without affecting other versions.
- Normal query execution never reads from a building, failed, or retired version unless an explicit admin/debug path requests it.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering version creation, active routing, failed build isolation, restart hydration, version drop cleanup, and metadata introspection.
- Include integration and catalog tests.
- Include tests proving versioned indexes/column batches/metadata do not collide where those structures are present.

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
