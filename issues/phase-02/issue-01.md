# Phase 02 Issue 01: Row Hashes

Milestone: Read-Model Core
Area: Verification
Status: Open
Priority: P1

## Requirements

Compute and persist deterministic row hashes for projection rows so rebuilds, diffs, and integrity checks can compare logical row state.
This issue establishes the canonical logical row digest contract used by the rest of phase 02.

## Dependencies

- Depends on phase 01 issue 03 for materialized projection row lifecycle and projection identity.
- Uses phase 01 issue 04 version identifiers when hashing versioned projections; collection/schema identity is sufficient for non-versioned rows.

## Handoff

- Provides the row-hash algorithm, metadata, persistence format, and internal API consumed by phase 02 issue 02 range hashes, phase 02 issue 04 rebuild verification, and phase 02 issue 06 integrity verification.

## Functional Scope

- Define a canonical row-hash input: projection id/version id where applicable, collection id, schema epoch, row id, active field ids, type tags, null/missing markers, and canonical encoded field values ordered by field id.
- Use a versioned hash algorithm with explicit metadata: algorithm name, digest length, canonical encoder version, and row-hash version.
- Compute/update row hashes on SQL inserts, updates, deletes, REST ingest, row blob rebuild, collection rename/drop, and startup hydration repair.
- Remove or tombstone hashes deterministically on logical delete so deleted rows are not reported as missing live hashes.
- Store hashes in Midge under versioned keys separate from row blobs while keeping row blobs authoritative.
- Expose row-hash availability, algorithm metadata, and recompute/repair diagnostics through an internal API plus metrics/catalog diagnostics.
- Provide an internal pure function that computes the expected digest from logical row state without writing storage, for verifiers and tests.

## Non-Goals

- Do not implement range hashes, Merkle roots, or rebuild verification in this issue.
- Do not add signatures, encryption, or tamper-proof remote attestation.
- Do not make row hash availability required for query correctness.
- Do not introduce an external event-store dependency.

## Acceptance Criteria

- Identical logical rows with the same schema epoch produce identical hashes across runs and restarts.
- Changes to any active field value, null/missing state, row id, or schema epoch change the row hash.
- Retired fields and physical row blob encoding differences do not change the logical row hash unless logical values change.
- Missing/corrupt hashes can be rebuilt from row blobs and are observable through diagnostics.
- Incompatible or unknown row-hash algorithm metadata is reported as unavailable/stale rather than treated as verified.
- Existing rows without row hashes continue to query correctly and can be backfilled deterministically.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering deterministic hashing, field changes, null/missing fields, schema epoch changes, retired fields, algorithm metadata, restart hydration, rebuild, backfill, and delete cleanup.
- Include integration coverage for SQL and REST write paths if both maintain hashes.

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
