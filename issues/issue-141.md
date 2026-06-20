# Issue 141: Projection Comparison

Milestone: V5 - Verification & Advanced Execution
Area: Distributed Read Models
Status: Open
Priority: P3

## Requirement

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

## Closeout Steps

- Run the validation commands below.
- Validate any additional touched test file before closing.
- Run `cargo build --locked`.
- Run `cargo fmt --all -- --check`.
- Document comparison report shape and compatibility rules.

## Validation

- `cargo test --test planner --quiet`
- `cargo test --test integration_sql --quiet`
- `cargo test --test metrics --quiet`
- `cntryl-tools validate-tests -f tests/planner.rs`
- `cntryl-tools validate-tests -f tests/integration_sql.rs`
- `cntryl-tools validate-tests -f tests/metrics.rs`
