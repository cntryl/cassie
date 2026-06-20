# Issue 077: Metadata Prefilters

Milestone: V2 - Query Performance
Area: Vector
Status: Open
Priority: P1

## Requirement

Apply scalar metadata predicates before vector scoring so search candidates can be narrowed without changing SQL or REST query results.

## Functional Scope

- Detect simple metadata filters on non-vector fields for vector and hybrid queries: equality, range, `IN`, `IS NULL`, and conjunctions already supported by the binder.
- Prefer existing scalar/composite indexes when a prefilter matches indexable predicates; otherwise use a row-scan prefilter before vector scoring.
- Preserve predicate semantics for nulls, missing sparse fields, type casts, and case-insensitive field resolution.
- Report prefilter activity through EXPLAIN and metrics, including input candidates, filtered candidates, and fallback reason when no prefilter is used.
- Keep full correctness fallback when predicates are unsupported, volatile, reference computed aliases, or require post-score filtering.

## Non-Goals

- Do not implement arbitrary predicate implication or join-aware prefilters in this issue.
- Do not change vector ranking semantics or the final ordering after LIMIT/OFFSET.

## Acceptance Criteria

- Vector and hybrid queries with metadata filters return the same rows as the unoptimized executor baseline.
- Matching scalar indexes reduce the vector candidate set before scoring and are visible in EXPLAIN/metrics.
- Unsupported filters fall back to existing behavior and emit no incorrect partial results.
- Prefilters work for SQL paths and REST vector/search APIs when equivalent metadata filters are available.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering indexed equality prefilter, range prefilter, null/missing-field behavior, unsupported predicate fallback, and hybrid search interaction.
- Include a metrics assertion that candidate counts decrease when prefiltering is selected.

## Closeout Steps

- Run the validation commands below.
- Run `cargo build --locked` before marking complete because this touches planner/executor contracts.
- Run `cargo fmt --all -- --check`.
- Update docs if EXPLAIN, REST filter syntax, or metrics fields change.

## Validation

- `cargo test --test integration_sql --quiet`
- `cargo test --test vector_index_metadata --quiet`
- `cntryl-tools validate-tests -f tests/integration_sql.rs`
