# Issue 105: HNSW Indexes

Milestone: V3 - Advanced Query Features
Area: Vector
Status: Open
Priority: P2

## Requirement

Support HNSW vector indexes as an optional approximate acceleration path for vector search while keeping row blobs and brute-force scoring as the correctness fallback.

## Functional Scope

- Add parser/binder/catalog support for HNSW vector indexes and options such as metric, dimensions, `m`, `ef_construction`, and query `ef_search` where exposed.
- Persist HNSW graph metadata and entries in Midge with versioned keys, and hydrate or rebuild after restart.
- Maintain graph membership on ingest, SQL writes, deletes, rebuilds, collection rename/drop, and vector index drop.
- Planner selects HNSW only for compatible vector top-k shapes, metric, dimensions, and optional metadata prefilters.
- Executor verifies candidates against stored row vectors before final ordering so returned distances/scores are exact for emitted rows.

## Non-Goals

- Do not guarantee exact nearest-neighbor recall for HNSW candidate generation.
- Do not remove brute-force vector search or require HNSW for vector queries.

## Acceptance Criteria

- HNSW index creation, persistence, hydration, rebuild, query, and drop work for cosine, dot, and L2 metrics where supported.
- Returned rows are sorted by exact distance/score after candidate verification.
- Unsupported metrics, dimensions, options, or query shapes fall back or fail with deterministic errors as appropriate.
- EXPLAIN and metrics identify HNSW use, candidate counts, fallback, and rebuild behavior.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering index creation/options, query selection, exact re-ranking, update/delete maintenance, restart hydration, rebuild, incompatible metric/dimension rejection, and fallback.
- Include vector metadata and SQL integration tests.

## Closeout Steps

- Run the validation commands below.
- Run `cargo build --locked` because this touches vector storage/planner/executor.
- Run `cargo fmt --all -- --check`.
- Add or update benchmarks for HNSW query and rebuild behavior.

## Validation

- `cargo test --test integration_sql --quiet`
- `cargo test --test vector_index_metadata --quiet`
- `cntryl-tools validate-tests -f tests/integration_sql.rs`
