# Sprint 10 - Full-Text Search Stack

Previous: [Sprint 09 - UDFs and Stored Procedures](sprint-09.md)  
Next: [Sprint 11 - Vector and Hybrid Retrieval](completed/sprint-11.md)

## Goal

Complete Cassie's V1 full-text search behavior over Midge-backed documents and expose deterministic search results through SQL functions.

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

- Complete tokenizer, lowercasing, stop-word filtering, BM25 scoring, snippets/highlights, field boosts, and post-filter behavior.
- Store and discover full-text index metadata through `cf0`.
- Execute full-text search over documents read from `cf1`.
- Expose `search(...)`, `search_score(...)`, and snippet behavior through SQL.
- Keep score ordering deterministic across repeated execution.
- Use stable tie-breakers when search scores are equal.
- Ensure field boosts are catalog-aware and survive restart when persisted.
- Keep search execution compatible with regular `WHERE`, `ORDER BY`, `LIMIT`, and `OFFSET` semantics.

## Acceptance Criteria

- Tokenization, stop words, BM25 ordering, snippets, field boosts, and filtered search tests pass.
- Repeated search queries return stable scores and ordering.
- Full-text metadata survives restart.
- `search(...)` can be used in filters.
- `search_score(...)` can be used in projections and order expressions.
- Snippet/highlight output is deterministic for matching text.
- `cargo build` passes.
- `cargo clippy --all-targets --all-features -- -D warnings` passes.
- All touched tests pass `cntryl-tools validate-tests`.

## Tests

- `tests/search_vector.rs`: tokenization, stop words, BM25 scoring, and snippet behavior.
- `tests/executor.rs`: `search(...)`, `search_score(...)`, and snippet functions through SQL.
- Add full-text metadata persistence tests alongside catalog storage tests.
- Add score tie-breaker tests before changing search ranking.
- Validate every touched test file with `cntryl-tools`.

## Exit Gate

This sprint is complete when full-text scoring, filtering, snippets, metadata persistence, and SQL function integration are covered by validator-clean tests, targeted search/executor tests pass, `cargo build` passes, and Clippy is clean with warnings denied.
