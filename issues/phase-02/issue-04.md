# Phase 02 Issue 04: Rebuild Verification

Milestone: Read-Model Core
Area: Verification
Status: Open
Priority: P1

## Requirements

Verify rebuilt indexes/projections against source row hashes and Merkle roots before marking rebuilds healthy or swappable.
This issue turns the phase 02 hash ladder into an activation gate for rebuilt read-model artifacts.

## Dependencies

- Depends on phase 01 issue 04 for versioned projection builds and target version identity.
- Depends on phase 01 issue 05 for active-version swap eligibility hooks.
- Depends on phase 02 issues 01, 02, and 03 for row hashes, range hashes, and projection Merkle roots.

## Handoff

- Provides verification status consumed by phase 02 issue 05 operations views and phase 02 issue 06 integrity verification.
- Provides the verification gate used by projection swaps when verification metadata is present.

## Functional Scope

- Add a rebuild verification phase that compares source row hashes, rebuilt row hashes, range hashes, and projection roots for compatible rebuild targets.
- Verify only compatible targets: matching projection definition, schema epoch, source checkpoint where available, and hash algorithm metadata.
- Record verification status, started/completed timestamps, mismatch counts, unverifiable ranges, and failure reason in catalog metadata.
- Track verification state as pending, running, verified, failed, unverifiable, or skipped.
- Block projection swaps or index activation when verification fails unless an explicit unsafe override exists and is tested.
- Support retry/resume after partial verification failure.
- Expose verification status through catalog/introspection, EXPLAIN/admin diagnostics, and metrics.
- Keep verification idempotent; rerunning verification over unchanged inputs must produce the same result metadata aside from timestamps.

## Non-Goals

- Do not repair corrupted data automatically.
- Do not require verification for features that do not yet produce hashes.
- Do not compare against remote Cassie instances or external event stores.

## Acceptance Criteria

- Successful rebuild verification marks the rebuilt target verified and eligible for activation.
- Mismatched, missing, stale, or incompatible hashes fail verification with deterministic diagnostics.
- Failed verification leaves the previous active projection/index state usable.
- Verification retry is idempotent after the underlying mismatch is corrected.
- Unverifiable targets are distinct from verified and failed targets in diagnostics and cannot be silently promoted as healthy.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering successful verification, row mismatch, missing hash, stale root, incompatible metadata, unverifiable target, failed swap blocking, retry after repair, restart hydration, and metrics.
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
