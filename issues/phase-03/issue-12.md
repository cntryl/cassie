# Phase 03 Issue 12: Analytical Projections

Milestone: Read-Model Analytics
Area: Advanced Analytics
Status: Open
Priority: P2

## Requirements

Support analytical projection definitions that materialize query-shaped read models optimized for scans, grouping, and search/vector analytics.
Analytical projections are derived read models; they may accelerate eligible queries only when freshness and correctness metadata prove they are safe.

## Dependencies

- Depends on phase 01 materialized projection lifecycle/versioning/swaps and phase 02 verification metadata when available.
- Consumes phase 03 issue 02 cost-informed planning, phase 03 issue 04 IVFFlat indexes, phase 03 issue 06 column-native execution, phase 03 issue 07 hybrid planning, and phase 03 issue 10 advanced statistics where implemented.

## Handoff

- Provides query-shaped derived read models consumed by phase 03 issue 13 large-scale aggregations and future operational reporting workflows.

## Functional Scope

- Build on materialized projections, projection versioning, column batches, rollups, and verification metadata.
- Allow analytical projections to declare source collections, selected/derived fields, deterministic expressions, filters, grouping keys, partition fields, sort keys, column storage options, and refresh policy.
- Persist definition fingerprints, source checkpoints, projection version ids, freshness/lag state, verification state where available, and dependency metadata.
- Maintain analytical projection data from source rows or compatible replay/refresh inputs and mark freshness/lag explicitly.
- Planner can route eligible analytical queries to projections only when fields, filters, aggregates, freshness, and correctness guarantees match.
- Expose projection metadata, definition fingerprint, freshness, verification state, selected use, and fallback through catalog views, EXPLAIN, and metrics.

## Non-Goals

- Do not make analytical projections required for query correctness.
- Do not support arbitrary external data sources or non-deterministic projection definitions.
- Do not silently use stale or unverifiable analytical projections when source execution can preserve semantics.

## Acceptance Criteria

- Analytical projections build, refresh, hydrate, query, and drop correctly.
- Eligible queries return identical results through analytical projections and source execution.
- Stale or incompatible projections fall back to source execution with diagnostics.
- Verification metadata can confirm projection build integrity when available.
- DML against analytical projection outputs is rejected unless it targets an allowed source write path.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering projection definition/fingerprint, deterministic expressions, build/refresh, query routing, stale/unverified fallback, DML rejection, restart hydration, drop cleanup, verification integration, and metrics.
- Include planner, integration, and catalog tests.

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
- `cargo test --locked --test planner_aggregates_sets --test planner_physical --test planner_estimates`
- `cargo test --locked --test integration_sql_aggregates --test integration_sql_fulltext_query --test integration_sql_hybrid_query`
- `cargo test --locked --test integration_sql_vector_indexes --test integration_sql_vector_query --test metrics_search --test metrics_adaptive`
- `cargo test --locked --test executor_parallel --test executor_vector_scoring --test rest_embeddings`
- `cargo test --locked`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
