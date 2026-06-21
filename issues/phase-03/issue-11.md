# Phase 03 Issue 11: Projection Comparison

Milestone: Read-Model Verification
Area: Distributed Read Models
Status: Open
Priority: P2

## Requirements

Compare two local projection versions or imported projection manifests and produce a deterministic consistency report.
Projection comparison packages local diff results and imported manifests into an operator-facing consistency report.

## Dependencies

- Depends on phase 03 issue 05 for local projection diffing.
- Depends on phase 02 issues 01, 02, and 03 for hash metadata compatibility and Merkle/range structures.
- Consumes phase 02 issue 05 operations diagnostics conventions for report state and persistence.

## Handoff

- Provides persisted comparison reports consumed by future distributed consistency and repair/export workflows.

## Functional Scope

- Accept local projection identifiers/versions or imported verification manifests containing source identity, schema/hash metadata, projection definition fingerprint, Merkle roots/ranges, row counts, and source checkpoint where available.
- Validate compatibility before comparison: database identity, collection identity, schema epoch, source checkpoint compatibility, hash algorithm, range fanout, and projection definition fingerprint.
- Use projection diffing to summarize equality, changed ranges, row counts, and unverifiable regions.
- Store comparison reports with report id, timestamp, inputs, compatibility status, mismatch counts, unverifiable regions, bounded diagnostic samples, and retention metadata.
- Expose reports through an admin/internal API, catalog diagnostics, and metrics.

## Non-Goals

- Do not contact remote Cassie instances directly in this issue; leave active multi-instance checks for a future distributed consistency issue.
- Do not repair differences automatically.
- Do not accept unsigned/untrusted manifests as proof of remote correctness; they are comparison inputs only.

## Acceptance Criteria

- Compatible identical projections compare equal using Merkle metadata.
- Changed projections produce deterministic mismatch summaries.
- Incompatible or stale manifests fail with explicit compatibility diagnostics.
- Reports persist and hydrate after restart until retention/cleanup removes them.
- Imported manifest parsing rejects missing required metadata without producing a false-equal report.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering equal comparison, mismatches, imported manifest compatibility, missing manifest metadata, incompatible metadata, stale/unverifiable ranges, bounded diagnostic samples, report persistence, restart hydration, and metrics.
- Include integration tests for the exposed comparison operation.

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
- `cargo test --locked --test integration_sql_catalog --test views --test catalog_introspection`
- `cargo test --locked --test midge_metadata_stats --test midge_namespace_hydration`
- `cargo test --locked --test metrics_runtime --test metrics_feedback`
- `cargo test --locked`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
