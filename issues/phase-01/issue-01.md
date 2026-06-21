# Phase 01 Issue 01: Projection Source Checkpoints

Milestone: Read-Model Core
Area: Projection Lifecycle
Status: Open
Priority: P0

## Requirements

Define and persist the source/checkpoint contract that ties a Cassie projection to the event-stream position it represents.
This issue establishes metadata only; later phase-01 issues consume the contract for replay ingestion, materialized projections, versioning, and swaps.

## Dependencies

- None. This is the first phase-01 implementation issue.

## Handoff

- Provides the checkpoint/freshness metadata contract consumed by phase 01 issue 02 replay ingestion and phase 01 issue 03 materialized projection freshness.

## Functional Scope

- Extend projection metadata with projection id, source identity, source checkpoint/position, last applied event id, replay batch id, lag, freshness, and last error.
- Define freshness values used by later issues: `unknown`, `fresh`, `stale`, `rebuilding`, and `failed`.
- Define checkpoint fields as opaque strings plus numeric ordering metadata where available so Cassie does not depend on a specific event-store implementation.
- Persist checkpoint metadata through Midge and hydrate it into the catalog during startup.
- Expose checkpoint state through catalog/admin diagnostics and runtime metrics.
- Keep checkpoint metadata separate from row blobs while preserving row blobs as the query correctness fallback.
- Define deterministic behavior for missing, stale, or incompatible checkpoint metadata.

## Non-Goals

- Do not implement replay ingestion in this issue; that is phase 01 issue 02.
- Do not implement materialized projection versioning or swaps in this issue.
- Do not introduce a second storage abstraction or external event-store dependency.
- Do not define a CNTRYL event-store wire protocol here.

## Acceptance Criteria

- A projection can report the event-stream source identity and source position it represents.
- Checkpoint metadata persists, hydrates after restart, and cleans up on drop/rename paths.
- Missing or invalid checkpoint metadata produces deterministic diagnostics rather than silently fresh state.
- Metrics and catalog/admin diagnostics expose lag, freshness, last applied event id, replay batch id, and last error where available.
- Existing collections without checkpoint metadata hydrate as `unknown` freshness without breaking ordinary SQL reads.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering metadata creation, source checkpoint update, restart hydration, missing metadata diagnostics, rename/drop cleanup, and metrics/catalog visibility.
- Include focused Midge/catalog tests and at least one integration-level diagnostics test.
- Add a compatibility test proving legacy projection metadata without source fields hydrates deterministically as `unknown`.

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
