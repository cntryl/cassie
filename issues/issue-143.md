# Issue 143: Multi-Instance Consistency Checks

Milestone: V5 - Verification & Advanced Execution
Area: Distributed Read Models
Status: Open
Priority: P3

## Requirement

Compare verification manifests from multiple Cassie instances to detect read-model divergence without performing replication or repair.

## Functional Scope

- Add an authenticated/admin-only path to export a projection verification manifest containing instance id, projection id/version, schema/hash metadata, root/range summaries, and generated timestamp.
- Add a consistency-check operation that imports two or more manifests and compares compatibility, roots, ranges, and optional row-level diffs.
- Report consistent, divergent, stale, incompatible, and unverifiable states with deterministic ordering.
- Store check reports locally and expose metrics for checks, mismatches, stale manifests, and incompatible manifests.
- Ensure exported manifests contain no row values or sensitive bind data.

## Non-Goals

- Do not implement data replication, leader election, quorum reads, or automatic repair.
- Do not require network calls from the query path.

## Acceptance Criteria

- Manifests from identical instances compare consistent.
- Divergent manifests report changed projections/ranges/rows where available.
- Stale, incompatible, or missing metadata reports clear non-success states.
- Manifest export/import and report persistence work after restart.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering manifest export, equal comparison, divergent comparison, stale manifest, incompatible schema/hash metadata, privacy/no row values, report persistence, and metrics.
- Include integration tests for admin/export/import paths.

## Closeout Steps

- Run the validation commands below.
- Validate any additional touched test file before closing.
- Run `cargo build --locked`.
- Run `cargo fmt --all -- --check`.
- Document manifest format, auth requirements, and non-repair semantics.

## Validation

- `cargo test --test planner --quiet`
- `cargo test --test integration_sql --quiet`
- `cargo test --test metrics --quiet`
- `cntryl-tools validate-tests -f tests/planner.rs`
- `cntryl-tools validate-tests -f tests/integration_sql.rs`
- `cntryl-tools validate-tests -f tests/metrics.rs`
