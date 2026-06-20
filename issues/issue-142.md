# Issue 142: Projection Integrity Verification

Milestone: V5 - Verification & Advanced Execution
Area: Distributed Read Models
Status: Open
Priority: P3

## Requirement

Verify that a local projection's rows, indexes, materialized state, and Merkle metadata are internally consistent.

## Functional Scope

- Add an integrity verification operation for a projection/collection/version that checks row blobs, row hashes, range hashes, root hash, scalar indexes, full-text/vector indexes, column batches, and materialized projection metadata where present.
- Produce a structured report with checked components, mismatches, missing entries, stale metadata, repairability, and elapsed time.
- Allow scoped verification modes: metadata-only, hashes-only, indexes-only, full verification.
- Store verification reports and expose status through admin/internal API, catalog diagnostics, and metrics.
- Keep verification read-only unless an explicit repair operation is implemented in a future issue.

## Non-Goals

- Do not auto-repair corruption or delete data.
- Do not compare against remote instances here.

## Acceptance Criteria

- Clean projections verify successfully across supported components.
- Injected/mocked missing index entries, stale hashes, or mismatched roots produce deterministic report entries.
- Verification can be scoped and reports skipped components explicitly.
- Verification failure does not affect normal query serving.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering successful full verification, missing row hash, stale root, missing index entry, scoped verification, report persistence, restart hydration, and metrics.
- Include integration tests for the exposed verification operation.

## Closeout Steps

- Run the validation commands below.
- Validate any additional touched test file before closing.
- Run `cargo build --locked`.
- Run `cargo fmt --all -- --check`.
- Document verification modes and report fields.

## Validation

- `cargo test --test planner --quiet`
- `cargo test --test integration_sql --quiet`
- `cargo test --test metrics --quiet`
- `cntryl-tools validate-tests -f tests/planner.rs`
- `cntryl-tools validate-tests -f tests/integration_sql.rs`
- `cntryl-tools validate-tests -f tests/metrics.rs`
