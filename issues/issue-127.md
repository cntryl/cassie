# Issue 127: Range Hashes

Milestone: V5 - Verification & Advanced Execution
Area: Merkle Overlay
Status: Open
Priority: P3

## Requirement

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

## Closeout Steps

- Run the validation commands below.
- Validate any additional touched test file before closing.
- Run `cargo build --locked`.
- Run `cargo fmt --all -- --check`.
- Document range boundaries, fanout, and storage key shape.

## Validation

- `cargo test --test integration_sql --quiet`
- `cntryl-tools validate-tests -f tests/integration_sql.rs`
