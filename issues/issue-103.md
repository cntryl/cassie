# Issue 103: Advanced Analyzers

Milestone: V3 - Advanced Query Features
Area: Search
Status: Open
Priority: P2

## Requirement

Add configurable full-text analyzers so indexing and query tokenization can use the same named analysis pipeline.

## Functional Scope

- Extend full-text index options with an `analyzer` name and analyzer-specific options such as case folding, stop-word set, stemming mode, and accent folding where implemented.
- Persist analyzer configuration in full-text index metadata and hydrate it after restart.
- Apply the same analyzer to indexing, `search()`, `search_score()`, snippet generation, BM25 statistics, rebuilds, and cached scoring metadata.
- Reject unknown analyzers or incompatible analyzer options during binding with deterministic errors.
- Invalidate or partition scoring metadata caches when analyzer configuration changes.

## Non-Goals

- Do not add user-defined tokenizer plugins in this issue; that is issue 104.
- Do not change the default analyzer behavior for existing indexes unless an analyzer is explicitly configured.

## Acceptance Criteria

- Full-text indexes with explicit analyzer options produce deterministic search results, scores, snippets, and restart behavior.
- Existing full-text indexes without analyzer options continue to behave as they do today.
- Rebuild and cache paths use analyzer-specific tokens and statistics.
- Unsupported analyzer names/options fail before index metadata is persisted.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering analyzer option parsing/binding, default compatibility, stop-word/stemming behavior where implemented, cache invalidation, restart hydration, and snippet consistency.
- Include executor and SQL integration coverage.

## Closeout Steps

- Run the validation commands below.
- Run `cargo build --locked` because this touches search metadata and execution.
- Run `cargo fmt --all -- --check`.
- Document supported analyzer names and options.

## Validation

- `cargo test --test integration_sql --quiet`
- `cargo test --test executor --quiet`
- `cntryl-tools validate-tests -f tests/integration_sql.rs`
