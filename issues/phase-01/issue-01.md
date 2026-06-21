# Phase 01 Issue 01: Projection Source Checkpoints

Milestone: Read-Model Core
Area: Projection Lifecycle
Status: Open
Priority: P0

## Requirements

Define and persist the source/checkpoint contract that ties a Cassie projection to the event-stream position it represents.

## Functional Scope

- Extend projection metadata with source identity, projection id, source checkpoint/position, last applied event id, replay batch id, lag, freshness, and last error.
- Persist checkpoint metadata through Midge and hydrate it into the catalog during startup.
- Expose checkpoint state through catalog/admin diagnostics and runtime metrics.
- Keep checkpoint metadata separate from row blobs while preserving row blobs as the query correctness fallback.
- Define deterministic behavior for missing, stale, or incompatible checkpoint metadata.

## Non-Goals

- Do not implement replay ingestion in this issue; that is issue 148.
- Do not implement materialized projection versioning or swaps in this issue.
- Do not introduce a second storage abstraction or external event-store dependency.

## Acceptance Criteria

- A projection can report the event-stream source identity and source position it represents.
- Checkpoint metadata persists, hydrates after restart, and cleans up on drop/rename paths.
- Missing or invalid checkpoint metadata produces deterministic diagnostics rather than silently fresh state.
- Metrics and catalog/admin diagnostics expose lag, freshness, last applied event id, replay batch id, and last error where available.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering metadata creation, source checkpoint update, restart hydration, missing metadata diagnostics, rename/drop cleanup, and metrics/catalog visibility.
- Include focused Midge/catalog tests and at least one integration-level diagnostics test.

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
- `cargo test --locked --test midge_metadata_stats --test catalog_introspection`
- `cargo test --locked --test metrics_runtime`
- `cargo test --locked`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
