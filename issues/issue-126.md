# Issue 126: Row Hashes

Milestone: V5 - Verification & Advanced Execution
Area: Merkle Overlay
Status: Open
Priority: P3

## Requirements

Compute and persist deterministic row hashes for projection rows so rebuilds, diffs, and integrity checks can compare logical row state.

## Functional Scope

- Define a canonical row-hash input: collection id, schema epoch, row id, active field ids, type tags, null/missing markers, and canonical encoded field values ordered by field id.
- Use a versioned hash algorithm with explicit metadata; the initial algorithm should be a 256-bit non-ambiguous digest suitable for Merkle trees.
- Compute/update row hashes on SQL inserts, updates, deletes, REST ingest, row blob rebuild, collection rename/drop, and startup hydration repair.
- Store hashes in Midge under versioned keys separate from row blobs while keeping row blobs authoritative.
- Expose row-hash availability and verification through an internal API plus metrics/catalog diagnostics.

## Non-Goals

- Do not add signatures, encryption, or tamper-proof remote attestation.
- Do not make row hash availability required for query correctness.

## Acceptance Criteria

- Identical logical rows with the same schema epoch produce identical hashes across runs and restarts.
- Changes to any active field value, null/missing state, row id, or schema epoch change the row hash.
- Retired fields and physical row blob encoding differences do not change the logical row hash unless logical values change.
- Missing/corrupt hashes can be rebuilt from row blobs and are observable through diagnostics.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering deterministic hashing, field changes, null/missing fields, schema epoch changes, retired fields, restart hydration, rebuild, and delete cleanup.
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
