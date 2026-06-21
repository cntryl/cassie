# Issue 128: Projection Merkle Roots

Milestone: V5 - Verification & Advanced Execution
Area: Merkle Overlay
Status: Open
Priority: P3

## Requirements

Compute projection-level Merkle roots from range hashes so a complete projection version can be verified with one digest.

## Functional Scope

- Define a root-hash input that includes projection/collection identity, schema epoch, hash algorithm version, range fanout version, and ordered child range hashes.
- Maintain roots after row/range hash updates, rebuilds, projection version builds, swaps, rename/drop, and startup hydration.
- Persist roots with version/state metadata and expose them through catalog/introspection, metrics, and internal verification APIs.
- Mark roots stale when required child hashes are missing or when source data changes before recomputation.
- Support empty projections with a deterministic root value.

## Non-Goals

- Do not compare roots across instances in this issue.
- Do not make roots a query-planning dependency.

## Acceptance Criteria

- Roots are deterministic across restarts and rebuilds for identical projection content.
- Any logical row change in the projection changes the root after recomputation.
- Stale/missing root state is observable and does not report false success.
- Projection versioning and swaps maintain separate roots per version.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering root creation, empty projection root, row-change propagation, stale state, restart hydration, rebuild, and projection-version isolation.
- Include catalog/metrics assertions where root state is exposed.

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
