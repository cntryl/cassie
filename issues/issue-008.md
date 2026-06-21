# Issue 008: Range Hashes

Milestone: Read-Model Core
Area: Verification
Status: Open
Priority: P1

## Requirements

Build deterministic range hashes over ordered row hashes so large projections can be compared without reading every row.

## Functional Scope

- Define range boundaries by collection/projection version and stable row-id ordering.
- Combine row hashes into fixed-size range nodes with versioned fanout/segment size metadata.
- Update affected range hashes when row hashes are inserted, updated, deleted, rebuilt, or repaired.
- Store range hash nodes in Midge with enough metadata to detect stale or missing child hashes.
- Expose range verification and diagnostics for projection diffing and integrity checks.

## Non-Goals

- Do not implement cross-instance comparison APIs here; projection diffing and distributed checks are later issues.
- Do not scan full row blobs when row hashes are current.

## Acceptance Criteria

- Range hashes are deterministic across restarts and rebuilds for the same ordered row hashes.
- Updating one row recomputes only affected ranges and parent nodes required by the chosen fanout.
- Missing or stale row hashes trigger rebuild/repair diagnostics rather than silently incorrect range hashes.
- Empty ranges and deleted rows have deterministic representations.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering range creation, row update propagation, delete propagation, empty ranges, restart hydration, rebuild repair, and deterministic fanout behavior.
- Include integration tests that compare range hashes before and after controlled data changes.

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
