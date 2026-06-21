# Phase 02 Issue 05: Projection Operations Views

Milestone: Read-Model Core
Area: Operations
Status: Open
Priority: P1

## Requirements

Expose projection operational state so users can diagnose which read model is being served, from which source position, and with what freshness and verification status.
This issue makes the phase 01 lifecycle and phase 02 verification metadata observable through stable operator-facing surfaces.

## Dependencies

- Depends on phase 01 issues 01 through 05 for checkpoint, freshness, rebuild, version, and swap metadata.
- Consumes phase 02 issues 01 through 04 metadata when hashes, roots, or rebuild verification are implemented.

## Handoff

- Provides the operational visibility used by phase 02 issue 06 integrity verification, phase 02 issue 07 performance target reporting, and phase 02 issue 08 mixed execution diagnostics.

## Functional Scope

- Add catalog/admin diagnostics for active version, source checkpoint, lag, freshness, rebuild state, verification state, last replay batch, last error, and fallback counters.
- Define stable state rendering for `unknown`, `fresh`, `stale`, `rebuilding`, `failed`, `pending`, `running`, `verified`, `unverifiable`, and `skipped` where those states apply.
- Extend runtime metrics with projection operation counters and last-state fields where needed.
- Update EXPLAIN or diagnostics when query execution reads a projection version, derived rollup, column batch, aggregate acceleration path, or fallback path.
- Ensure diagnostics hydrate correctly after restart when persisted metadata exists.
- Prefer PostgreSQL-visible catalog/introspection surfaces for query tooling; keep REST/admin diagnostics secondary.
- Keep metric labels bounded so projection operations metrics do not create unbounded cardinality.

## Non-Goals

- Do not implement projection versioning, swaps, or verification in this issue; consume their metadata when present.
- Do not create a separate observability backend.
- Do not mark optional metadata as fresh or verified when the producing feature has not been implemented.

## Acceptance Criteria

- Operators can inspect projection checkpoint, freshness, rebuild, verification, and active-version state through documented surfaces.
- Missing optional metadata is reported explicitly as unknown or unavailable, not fresh.
- Restart hydration preserves visible projection operations state.
- Query diagnostics expose derived-state use or fallback where it affects performance or freshness.
- Metrics and diagnostics use the same state vocabulary for the same condition.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering catalog view output, metrics snapshot output, unknown/unavailable-state diagnostics, restart hydration, bounded metric labels, and EXPLAIN/fallback visibility.
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
