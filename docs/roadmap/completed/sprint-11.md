# Sprint 11 - Vector and Hybrid Retrieval

Previous: [Sprint 10 - Full-Text Search Stack](sprint-10.md)  
Next: [Sprint 12 - Runtime Observability, Plan Cache, and Operational Controls](../sprint-12.md)

## Goal

Finish V1 vector and hybrid retrieval with brute-force execution, deterministic ranking, schema validation, and pgvector-inspired SQL compatibility.

## Invariants

- TDD first: add or update single-behavior tests before implementation.
- All touched tests use `should_` names plus `// Arrange`, `// Act`, `// Assert`.
- Validate touched tests with `cntryl-tools validate-tests -f <file>`.
- Keep Midge direct; no second storage abstraction.
- Preserve Midge family contract: `cf0` metadata/schema/config, `cf1` documents/data, `cf2` temp, `default` engine-reserved.
- Keep REST secondary and PostgreSQL wire primary.
- No Axum and no third-party SQL parser.
- Unsupported behavior returns deterministic `CassieError` or PostgreSQL-style wire errors.
- Each sprint exits only when targeted tests are green, touched tests pass `cntryl-tools validate-tests`, `cargo build` passes, and `cargo clippy --all-targets --all-features -- -D warnings` passes.
- Release sprints also run full `cargo test`.

## Requirements

- Implement vector schema validation for `vector(N)` fields.
- Validate vector dimensions on ingest and query.
- Support cosine distance, L2 distance, dot product, vector score, and vector distance functions.
- Support brute-force retrieval as the V1 vector execution strategy.
- Support metadata filters alongside vector ordering.
- Support pgvector-style order operators `<=>`, `<->`, and `<#>`.
- Persist vector index metadata through `cf0`.
- Keep HNSW and IVFFlat as future extension points only.
- Implement deterministic `hybrid_score(...)` fusion with stable tie-breakers.

## Acceptance Criteria

- Vector dimension mismatches fail at write and query paths.
- Function/operator parity tests pass for `<=>`, `<->`, and `<#>`.
- Hybrid ranking is deterministic across repeated runs.
- Vector index metadata persists through restart.
- Brute-force vector ordering is stable under equal scores.
- Metadata filters compose with vector ranking.
- `cargo build` passes.
- `cargo clippy --all-targets --all-features -- -D warnings` passes.
- All touched tests pass `cntryl-tools validate-tests`.

## Tests

- `tests/search_vector.rs`: vector math and hybrid scoring primitives.
- `tests/executor.rs`: vector functions, pgvector-style operators, dimension mismatch, and hybrid ordering through SQL.
- `tests/embedding_validation.rs`: provider, model, metric, and dimension mismatch behavior.
- `tests/vector_index_metadata.rs`: vector metadata persistence and reload.
- `tests/rest_embeddings.rs`: ingest and vector search through REST where applicable.

## Exit Gate

This sprint is complete when vector and hybrid behavior is deterministic, all touched tests are validator-clean, targeted vector, executor, metadata, and embedding tests pass, `cargo build` passes, and Clippy is clean with warnings denied.
