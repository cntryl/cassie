# Issue 129: Projection Diffing

Milestone: V5 - Verification & Advanced Execution
Area: Merkle Overlay
Status: Open
Priority: P3

## Requirement

Compare two projection versions or collections using Merkle roots/ranges and report deterministic changed ranges and row ids.

## Functional Scope

- Add an internal and administrative diff operation that accepts two projection identifiers or versions with compatible hash algorithm metadata.
- Compare roots first, descend through range hashes only where hashes differ, and optionally resolve differing ranges to row ids and row-hash pairs.
- Return added, removed, changed, and unverifiable ranges/rows with deterministic ordering.
- Handle missing/stale hashes by reporting `unverifiable` instead of claiming equality.
- Expose diff counters and elapsed time through metrics.

## Non-Goals

- Do not repair differences automatically in this issue.
- Do not compare incompatible schema epochs unless an explicit compatibility layer exists.

## Acceptance Criteria

- Identical projections diff as equal without scanning all rows.
- Controlled added, removed, and changed rows are reported accurately and deterministically.
- Missing/stale hash data is reported as unverifiable.
- Diffing works across projection versions after restart hydration.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering equal roots, added rows, removed rows, changed rows, mixed range differences, stale hashes, incompatible schemas, and restart hydration.
- Include integration tests for the exposed admin/internal operation.

## Closeout Steps

- Run the validation commands below.
- Validate any additional touched test file before closing.
- Run `cargo build --locked`.
- Run `cargo fmt --all -- --check`.
- Document diff result shape and compatibility rules.

## Validation

- `cargo test --test integration_sql --quiet`
- `cntryl-tools validate-tests -f tests/integration_sql.rs`
