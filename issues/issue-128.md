# Issue 128: Projection Merkle Roots

Milestone: V5 - Verification & Advanced Execution
Area: Merkle Overlay
Status: Open
Priority: P3

## Requirement

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

## Closeout Steps

- Run the validation commands below.
- Validate any additional touched test file before closing.
- Run `cargo build --locked`.
- Run `cargo fmt --all -- --check`.
- Document root input and stale-state semantics.

## Validation

- `cargo test --test integration_sql --quiet`
- `cntryl-tools validate-tests -f tests/integration_sql.rs`
