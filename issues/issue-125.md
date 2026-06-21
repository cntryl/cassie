# Issue 125: IVFFlat Indexes

Milestone: V4 - Analytical Overlay
Area: Vector
Status: Open
Priority: P3

## Requirements

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
