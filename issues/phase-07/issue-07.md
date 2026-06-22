# Phase 07 Issue 07: Multi-Instance Consistency Checks

Milestone: Advanced Backlog
Area: Distributed Read Models
Status: Open
Priority: P3

## Requirements

Compare verification manifests from multiple Cassie instances to detect read-model divergence without performing replication or repair.
This issue extends local verification and projection comparison into an offline/admin consistency workflow across instances.

## Dependencies

- Depends on phase 02 issues 01 through 06 for row hashes, range hashes, Merkle roots, rebuild verification, operations views, and local integrity reports.
- Depends on phase 03 issue 05 for projection diffing.
- Depends on phase 03 issue 11 for projection comparison semantics.
- Depends on the archived phase 04 REST blocking-boundary contract surface in `docs/performance-contracts.md` and `issues/phase-04/README.md` if manifest export/import is exposed through REST.
- Depends on the archived phase 05 write-amplification diagnostic surface in `docs/performance-contracts.md` and `issues/phase-05/README.md` when reports include operational counters.

## Handoff

- Provides divergence reports for future repair, audit, or deployment tooling without adding distributed query semantics.

## Functional Scope

- Add an authenticated/admin-only path to export a projection verification manifest containing instance id, projection id/version, schema/hash metadata, root/range summaries, and generated timestamp.
- Add a consistency-check operation that imports two or more manifests and compares compatibility, roots, ranges, and optional row-level diffs.
- Report consistent, divergent, stale, incompatible, and unverifiable states with deterministic ordering.
- Store check reports locally and expose metrics for checks, mismatches, stale manifests, and incompatible manifests.
- Ensure exported manifests contain no row values or sensitive bind data.
- Define a versioned manifest format with canonical ordering, manifest digest, hash algorithm metadata, source checkpoint where available, and expiration/staleness rules.
- Keep manifest export/import administrative and blocking-boundary compliant when called through async interfaces.
- Keep comparison offline/admin-driven; query planning and query execution must never wait on remote manifest checks.

## Non-Goals

- Do not implement data replication, leader election, quorum reads, or automatic repair.
- Do not require network calls from the query path.
- Do not treat manifest equality as proof of source-of-record correctness; it only compares Cassie read-model materialization state.

## Implementation Plan

### Step 1: Define manifest schema and policy

- Add versioned manifest format with canonical fields:
  - instance metadata
  - projection/schema identity
  - checkpoint or source epoch
  - range/range-hash summaries
- Include deterministic ordering and digest strategy.
- Define compatibility and expiration policy.

### Step 2: Add admin export path

- Add authenticated admin endpoint/API for manifest export.
- Include privacy filters to remove row values, vector payloads, text bodies, and credentials.
- Serialize manifest deterministically for stable file comparison.

### Step 3: Add manifest comparison path

- Add import/compare routine for two or more manifests.
- Produce structured result states: consistent, divergent, stale, incompatible, unverifiable.
- Keep comparison offline and non-blocking for query-critical paths.

### Step 4: Persist and expose report history

- Store report artifacts in Midge-backed metadata with retention and overwrite rules.
- Add simple query/admin access to latest report per projection and stale report counts.
- Ensure restart rehydrates compare history safely.

### Step 5: Diagnostics and governance

- Add metrics for export/import/check outcomes and mismatch counts.
- Add clear errors for mismatch reasons and incompatible schema/hash/collision fields.
- Add privacy and access audits to prevent sensitive row leakage in manifests.

### Step 6: Validation and close-out

- Add integration tests for export/import workflows, canonical ordering, privacy filtering, stale/incompatible states, and restart hydration.
- Add deterministic tests for mismatch reporting and report persistence.
- Update docs/README references for admin workflow behavior and limitations.

## Acceptance Criteria

- Manifests from identical instances compare consistent.
- Divergent manifests report changed projections/ranges/rows where available.
- Stale, incompatible, or missing metadata reports clear non-success states.
- Manifest export/import and report persistence work after restart.
- Manifest export excludes row values, vector values, full text bodies, bind values, and credentials.
- Compatibility checks reject mismatched schema epoch, hash algorithm, projection definition, or source checkpoint where those fields are required.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering manifest export, canonical ordering, equal comparison, divergent comparison, stale manifest, incompatible schema/hash/source metadata, privacy/no row values, report persistence, restart hydration, and metrics.
- Include integration tests for admin/export/import paths.

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
