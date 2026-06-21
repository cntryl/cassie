# Phase 03 Issue 04: IVFFlat Indexes

Milestone: Read-Model Retrieval
Area: Vector
Status: Open
Priority: P2

## Requirements

Support IVFFlat vector indexes as an optional approximate candidate-generation path with exact re-ranking before results are returned.
IVFFlat improves candidate generation only; final SQL-visible ordering must come from exact vector scoring.

## Dependencies

- Depends on existing vector distance metrics, vector index metadata, row blob vector storage, and planner vector top-k support.
- Consumes phase 03 issue 02 cost-informed planning and phase 03 issue 03 index feedback when those are available.

## Handoff

- Provides a versioned approximate vector access path consumed by phase 03 issue 12 analytical projections and phase 03 issue 13 large-scale aggregation/retrieval workloads where vector prefiltering is safe.

## Functional Scope

- Add parser/binder/catalog support for IVFFlat vector indexes and options such as metric, dimensions, `lists`, training sample size, training seed/version, and query `probes`.
- Build deterministic centroids/lists from row blob vectors and persist versioned centroid metadata, list metadata, training coverage, and row memberships in Midge.
- Maintain index memberships for compatible writes where supported, or mark IVFFlat indexes stale after writes, deletes, rebuilds, restart hydration, collection rename/drop, and index drop.
- Planner selects IVFFlat for compatible vector top-k shapes, metric, dimensions, and optional metadata prefilters.
- Executor probes configured lists, expands candidates when needed, fetches candidate row vectors, and re-ranks exactly before applying final ORDER BY, LIMIT, and OFFSET.
- Expose training state, stale state, lists/probes, candidate counts, exact re-rank counts, and fallback through EXPLAIN/metrics.

## Non-Goals

- Do not guarantee exact nearest-neighbor recall from IVFFlat candidate generation.
- Do not remove brute-force or HNSW vector paths.
- Do not return approximate scores or skip exact row-vector verification.

## Acceptance Criteria

- IVFFlat index creation, training/build, persistence, hydration, query, rebuild, and drop are covered.
- Returned rows are sorted by exact score/distance after candidate verification.
- Invalid options, incompatible dimensions/metrics, and stale indexes produce deterministic fallback or errors.
- EXPLAIN and metrics identify lists/probes, candidates, exact re-rank count, and fallback.
- Training metadata prevents use of incompatible or incomplete IVFFlat structures.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering create/options, deterministic training/build, query selection, candidate expansion, exact re-ranking, stale-write behavior, restart hydration, rebuild, invalid options, incompatible metadata, and fallback.
- Include vector metadata and SQL integration tests.

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
- `cargo test --locked --test parser_indexes --test planner_indexes --test planner_physical`
- `cargo test --locked --test vector_index_metadata --test integration_sql_vector_indexes --test integration_sql_vector_query`
- `cargo test --locked --test executor_vector_scoring --test rest_embeddings --test search_vector`
- `cargo test --locked`
- `cargo fmt --all -- --check`
- `cntryl-tools validate-tests -f <each touched test file>`
