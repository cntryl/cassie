# Issue 076: Normalized Vector Storage

Milestone: V2 - Query Performance
Area: Vector
Status: Closed
Priority: P1

## Requirement

Persist a normalized representation for vector fields/indexes so cosine and dot-product ranking can avoid repeated norm work while preserving existing query results.

## Functional Scope

- Keep the original vector value as the source of truth in row blobs and SQL/REST results.
- Add versioned persisted metadata for normalized vectors: field name, dimensions, metric, normalization version, and whether a normalized payload is available.
- Compute normalized payloads during SQL inserts, REST ingest, updates, and index rebuilds when the vector is finite and dimensions match the declared field/index.
- Use normalized payloads only for compatible cosine/dot vector search and scoring paths; L2 and unsupported shapes continue using the existing raw-vector path.
- Hydrate normalized-vector metadata after restart and rebuild missing normalized payloads from row blobs without changing row IDs or user-visible values.

## Non-Goals

- Do not introduce a second storage abstraction or replace row blob storage.
- Do not change pgvector operator semantics, result ordering, or REST response shapes.
- Do not silently accept invalid dimensions or non-finite vector values.

## Acceptance Criteria

- Cosine and dot-product `ORDER BY`, `vector_score`, and pgvector operator queries return the same rows and deterministic tie order as the raw-vector baseline.
- Restart and rebuild paths preserve normalized-vector metadata and continue to answer equivalent vector queries.
- Missing or incompatible normalized payloads fall back to raw row-vector scoring without incorrect results.
- Dimension, metric, provider, and model validation errors remain explicit and unchanged for callers.
- EXPLAIN or metrics identify when normalized vector storage is used versus fallback.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering normalized cosine/dot search, fallback when normalized payloads are missing, restart hydration, rebuild, and dimension mismatch rejection.
- Include at least one SQL integration test and one vector metadata persistence test.

## Closeout Steps

- Run the validation commands below.
- Run `cargo build --locked` before marking complete because this touches storage/executor contracts.
- Run `cargo fmt --all -- --check`.
- Update docs if any SQL, REST, EXPLAIN, or metrics surface changes.

## Validation

- `cargo test --test integration_sql --quiet`
- `cargo test --test vector_index_metadata --quiet`
- `cntryl-tools validate-tests -f tests/integration_sql.rs`
