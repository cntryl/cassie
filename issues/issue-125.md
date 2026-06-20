# Issue 125: IVFFlat Indexes

Milestone: V4 - Analytical Overlay
Area: Vector
Status: Open
Priority: P3

## Requirement

Support IVFFlat vector indexes as an optional approximate candidate-generation path with exact re-ranking before results are returned.

## Functional Scope

- Add parser/binder/catalog support for IVFFlat vector indexes and options such as metric, dimensions, `lists`, training sample size, and query `probes`.
- Build deterministic centroids/lists from row blob vectors and persist versioned list metadata and row memberships in Midge.
- Maintain or mark IVFFlat indexes stale after writes, deletes, rebuilds, restart hydration, collection rename/drop, and index drop.
- Planner selects IVFFlat for compatible vector top-k shapes, metric, dimensions, and optional metadata prefilters.
- Executor probes configured lists, fetches candidate row vectors, and re-ranks exactly before applying LIMIT/OFFSET.

## Non-Goals

- Do not guarantee exact nearest-neighbor recall from IVFFlat candidate generation.
- Do not remove brute-force or HNSW vector paths.

## Acceptance Criteria

- IVFFlat index creation, training/build, persistence, hydration, query, rebuild, and drop are covered.
- Returned rows are sorted by exact score/distance after candidate verification.
- Invalid options, incompatible dimensions/metrics, and stale indexes produce deterministic fallback or errors.
- EXPLAIN and metrics identify lists/probes, candidates, exact re-rank count, and fallback.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering create/options, training/build, query selection, exact re-ranking, stale-write behavior, restart hydration, rebuild, invalid options, and fallback.
- Include vector metadata and SQL integration tests.

## Closeout Steps

- Run the validation commands below.
- Run `cargo build --locked`.
- Run `cargo fmt --all -- --check`.
- Add benchmark evidence for IVFFlat build/query tradeoffs.

## Validation

- `cargo test --test integration_sql --quiet`
- `cargo test --test vector_index_metadata --quiet`
- `cntryl-tools validate-tests -f tests/integration_sql.rs`
