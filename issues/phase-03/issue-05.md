# Phase 03 Issue 05: Projection Diffing

Milestone: Read-Model Verification
Area: Diffing
Status: Open
Priority: P2

## Requirements

Compare two projection versions or collections using Merkle roots/ranges and report deterministic changed ranges and row ids.
This issue is the local diff primitive built on the phase 02 hash ladder; it reports differences but does not repair them.

## Dependencies

- Depends on phase 02 issues 01, 02, and 03 for row hashes, range hashes, projection Merkle roots, and hash metadata.
- Consumes phase 02 issue 05 operations diagnostics conventions for report status and metrics.

## Handoff

- Provides deterministic diff results consumed by phase 03 issue 11 projection comparison and future repair/export workflows.

## Functional Scope

- Add an internal and administrative diff operation that accepts two projection identifiers or versions with compatible projection definitions, schema epochs, source checkpoints where available, and hash algorithm metadata.
- Compare roots first, descend through range hashes only where hashes differ, and optionally resolve differing ranges to row ids and row-hash pairs.
- Return added, removed, changed, and unverifiable ranges/rows with deterministic ordering.
- Handle missing/stale hashes by reporting `unverifiable` instead of claiming equality.
- Support bounded output with deterministic resume/cursor metadata for large diffs.
- Expose diff counters, skipped ranges, unverifiable regions, and elapsed time through metrics and admin diagnostics.

## Non-Goals

- Do not repair differences automatically in this issue.
- Do not compare incompatible schema epochs unless an explicit compatibility layer exists.
- Do not contact remote Cassie instances or import remote manifests here.

## Acceptance Criteria

- Identical projections diff as equal without scanning all rows.
- Controlled added, removed, and changed rows are reported accurately and deterministically.
- Missing/stale hash data is reported as unverifiable.
- Diffing works across projection versions after restart hydration.
- Large diffs can be bounded without changing deterministic ordering or losing resume information.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering equal roots, added rows, removed rows, changed rows, mixed range differences, bounded/resumed output, stale hashes, incompatible schemas, incompatible hash metadata, and restart hydration.
- Include integration tests for the exposed admin/internal operation.

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
- `cargo test --locked --test integration_sql_catalog --test integration_sql_projection --test views`
- `cargo test --locked --test midge_metadata_stats --test midge_namespace_hydration --test midge_row_blob_layout`
- `cargo test --locked --test metrics_runtime --test vector_index_metadata`
- `cargo test --locked`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
