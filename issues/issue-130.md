# Issue 130: Rebuild Verification

Milestone: V5 - Verification & Advanced Execution
Area: Merkle Overlay
Status: Open
Priority: P3

## Requirement

Verify rebuilt indexes/projections against source row hashes and Merkle roots before marking rebuilds healthy or swappable.

## Functional Scope

- Add a rebuild verification phase that compares source row hashes, rebuilt row hashes, range hashes, and projection roots for compatible rebuild targets.
- Record verification status, started/completed timestamps, mismatch counts, unverifiable ranges, and failure reason in catalog metadata.
- Block projection swaps or index activation when verification fails unless an explicit unsafe override exists and is tested.
- Support retry/resume after partial verification failure.
- Expose verification status through catalog/introspection, EXPLAIN/admin diagnostics, and metrics.

## Non-Goals

- Do not repair corrupted data automatically.
- Do not require verification for features that do not yet produce hashes.

## Acceptance Criteria

- Successful rebuild verification marks the rebuilt target verified and eligible for activation.
- Mismatched, missing, stale, or incompatible hashes fail verification with deterministic diagnostics.
- Failed verification leaves the previous active projection/index state usable.
- Verification retry is idempotent after the underlying mismatch is corrected.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering successful verification, row mismatch, missing hash, stale root, failed swap blocking, retry after repair, restart hydration, and metrics.
- Include integration tests around rebuild/activation flows.

## Closeout Steps

- Run the validation commands below.
- Validate any additional touched test file before closing.
- Run `cargo build --locked`.
- Run `cargo fmt --all -- --check`.
- Document verification states and activation rules.

## Validation

- `cargo test --test integration_sql --quiet`
- `cntryl-tools validate-tests -f tests/integration_sql.rs`
