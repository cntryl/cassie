# Issue 146: Mixed Search / Vector / Analytical Execution

Milestone: V5 - Verification & Advanced Execution
Area: Advanced Analytics
Status: Open
Priority: P3

## Requirements

Plan and execute queries that combine full-text search, vector scoring, metadata filters, and analytical aggregation/projection paths with exact final results.

## Functional Scope

- Support mixed plans involving `search()`, `search_score()`, vector distance/score expressions, hybrid scoring, scalar filters, GROUP BY/HAVING, ORDER BY, LIMIT/OFFSET, column batches, and analytical projections.
- Use candidate generation, metadata prefilters, analytical projections, column scans, and exact re-ranking only when each stage preserves query semantics.
- Define stage ordering explicitly: candidate generation/prefiltering, exact scoring, analytical grouping/aggregation, ordering, offset, and limit.
- Fall back to source row execution when freshness, coverage, scoring semantics, or aggregate semantics are not compatible.
- Report stage selection, candidates, exact scoring rows, aggregate groups, projection freshness, and fallback through EXPLAIN/metrics.

## Non-Goals

- Do not approximate final scores or aggregate results.
- Do not silently use stale analytical projections.

## Acceptance Criteria

- Mixed search/vector/analytical queries return the same rows, scores, aggregates, and deterministic tie order as an exhaustive exact baseline.
- Planner refuses or falls back for incompatible projection freshness, unsupported score expressions, or uncovered fields.
- Candidate sizing and exact re-ranking happen before final ORDER BY/LIMIT where required by semantics.
- Metrics expose enough detail to diagnose each mixed execution stage.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering text+vector+filter top-k, hybrid score with aggregation, analytical projection routing, stale fallback, exact baseline comparison, candidate expansion, deterministic tie order, and EXPLAIN/metrics diagnostics.
- Include planner, integration, and metrics tests.

## Close-Out Steps

- Confirm every requirement and acceptance criterion above is implemented and covered by tests.
- Keep source, test, and benchmark files under 1,000 lines; split focused modules/tests before adding large blocks.
- Keep new code in the owning subsystem shown in `AGENTS.md` and `docs/module_organization.md`; do not introduce a second storage abstraction.
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
