# Phase 02 Issue 05: Projection Operations Views

Milestone: Read-Model Core
Area: Operations
Status: Open
Priority: P1

## Requirements

Expose projection operational state so users can diagnose which read model is being served, from which source position, and with what freshness and verification status.

## Functional Scope

- Add catalog/admin diagnostics for active version, source checkpoint, lag, freshness, rebuild state, verification state, last replay batch, last error, and fallback counters.
- Extend runtime metrics with projection operation counters and last-state fields where needed.
- Update EXPLAIN or diagnostics when query execution reads a projection version, derived rollup, column batch, aggregate acceleration path, or fallback path.
- Ensure diagnostics hydrate correctly after restart when persisted metadata exists.

## Non-Goals

- Do not implement projection versioning, swaps, or verification in this issue; consume their metadata when present.
- Do not create a separate observability backend.

## Acceptance Criteria

- Operators can inspect projection checkpoint, freshness, rebuild, verification, and active-version state through documented surfaces.
- Missing optional metadata is reported explicitly as unknown or unavailable, not fresh.
- Restart hydration preserves visible projection operations state.
- Query diagnostics expose derived-state use or fallback where it affects performance or freshness.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering catalog view output, metrics snapshot output, unknown-state diagnostics, restart hydration, and EXPLAIN/fallback visibility.
- Include pgwire-visible catalog query coverage where applicable.

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
- `cargo test --locked --test catalog_introspection --test metrics_runtime`
- `cargo test --locked --test integration_sql_explain --test time_series_rollups`
- `cargo test --locked`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
