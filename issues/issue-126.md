# Issue 126: Row Hashes

Milestone: V5 - Verification & Advanced Execution
Area: Merkle Overlay
Status: Open
Priority: P3

## Requirement

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

## Closeout Steps

- Run the validation commands below.
- Validate any additional touched test file before closing.
- Run `cargo build --locked`.
- Run `cargo fmt --all -- --check`.
- Document hash input, hash version, and compatibility guarantees.

## Validation

- `cargo test --test integration_sql --quiet`
- `cntryl-tools validate-tests -f tests/integration_sql.rs`
