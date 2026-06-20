# Issue 120: Projection Versioning

Milestone: V4 - Analytical Overlay
Area: Materialization
Status: Open
Priority: P3

## Requirement

Version materialized projection definitions and storage so new projection builds can coexist with the currently active version.

## Functional Scope

- Store projection versions with definition fingerprint, source schema epochs, output schema, storage prefix, build state, created timestamp, and active/retired markers.
- Route reads to exactly one active version unless an explicit admin/debug path requests another version.
- Allow a new version to build from source rows without corrupting or replacing the active version.
- Keep versioned index, column, and metadata entries isolated by projection version.
- Expose active, building, failed, and retired versions through catalog/introspection and metrics.

## Non-Goals

- Do not implement atomic active-version swaps here; that is issue 121.
- Do not support concurrent writes directly into projection output versions.

## Acceptance Criteria

- Multiple projection versions can exist without key collisions or mixed reads.
- Restart hydration preserves version state and active-version routing.
- Failed builds leave the previous active version readable.
- Dropping a version cleans up its storage without affecting other versions.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering version creation, active routing, failed build isolation, restart hydration, version drop cleanup, and metadata introspection.
- Include integration and catalog tests.

## Closeout Steps

- Run the validation commands below.
- Run `cargo build --locked`.
- Run `cargo fmt --all -- --check`.
- Document version states and storage key invariants.

## Validation

- `cargo test --test integration_sql --quiet`
- `cargo test --test catalog_introspection --quiet`
- `cntryl-tools validate-tests -f tests/integration_sql.rs`
