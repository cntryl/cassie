# Issue 142: Projection Integrity Verification

Milestone: V5 - Verification & Advanced Execution
Area: Distributed Read Models
Status: Open
Priority: P3

## Requirements

Verify that a local projection's rows, indexes, materialized state, and Merkle metadata are internally consistent.

## Functional Scope

- Add an integrity verification operation for a projection/collection/version that checks row blobs, row hashes, range hashes, root hash, scalar indexes, full-text/vector indexes, column batches, and materialized projection metadata where present.
- Produce a structured report with checked components, mismatches, missing entries, stale metadata, repairability, and elapsed time.
- Allow scoped verification modes: metadata-only, hashes-only, indexes-only, full verification.
- Store verification reports and expose status through admin/internal API, catalog diagnostics, and metrics.
- Keep verification read-only unless an explicit repair operation is implemented in a future issue.

## Non-Goals

- Do not auto-repair corruption or delete data.
- Do not compare against remote instances here.

## Acceptance Criteria

- Clean projections verify successfully across supported components.
- Injected/mocked missing index entries, stale hashes, or mismatched roots produce deterministic report entries.
- Verification can be scoped and reports skipped components explicitly.
- Verification failure does not affect normal query serving.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering successful full verification, missing row hash, stale root, missing index entry, scoped verification, report persistence, restart hydration, and metrics.
- Include integration tests for the exposed verification operation.

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
