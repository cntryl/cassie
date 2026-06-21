# Issue 010: Rebuild Verification

Milestone: Read-Model Core
Area: Verification
Status: Open
Priority: P1

## Requirements

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
