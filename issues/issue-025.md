# Issue 025: Projection Comparison

Milestone: Read-Model Verification
Area: Distributed Read Models
Status: Open
Priority: P2

## Requirements

Compare two local projection versions or imported projection manifests and produce a deterministic consistency report.

## Functional Scope

- Accept local projection identifiers/versions or imported verification manifests containing schema/hash metadata and Merkle roots/ranges.
- Validate compatibility before comparison: schema epoch, hash algorithm, range fanout, projection definition fingerprint, and collection identity.
- Use projection diffing to summarize equality, changed ranges, row counts, and unverifiable regions.
- Store comparison reports with timestamp, inputs, status, mismatch counts, and diagnostic samples.
- Expose reports through an admin/internal API, catalog diagnostics, and metrics.

## Non-Goals

- Do not contact remote Cassie instances directly in this issue; multi-instance checks are issue 143.
- Do not repair differences automatically.

## Acceptance Criteria

- Compatible identical projections compare equal using Merkle metadata.
- Changed projections produce deterministic mismatch summaries.
- Incompatible or stale manifests fail with explicit compatibility diagnostics.
- Reports persist and hydrate after restart until retention/cleanup removes them.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering equal comparison, mismatches, incompatible metadata, stale/unverifiable ranges, report persistence, restart hydration, and metrics.
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
