# Issue 146: Mixed Search / Vector / Analytical Execution

Milestone: V5 - Verification & Advanced Execution
Area: Advanced Analytics
Status: Open
Priority: P3

## Requirement

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

## Closeout Steps

- Run the validation commands below.
- Validate any additional touched test file before closing.
- Run `cargo build --locked`.
- Run `cargo fmt --all -- --check`.
- Add benchmark evidence for representative mixed workloads.

## Validation

- `cargo test --test planner --quiet`
- `cargo test --test integration_sql --quiet`
- `cargo test --test metrics --quiet`
- `cntryl-tools validate-tests -f tests/planner.rs`
- `cntryl-tools validate-tests -f tests/integration_sql.rs`
- `cntryl-tools validate-tests -f tests/metrics.rs`
