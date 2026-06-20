# Issue 104: Custom Tokenizers

Milestone: V3 - Advanced Query Features
Area: Search
Status: Open
Priority: P2

## Requirement

Provide a bounded tokenizer registry for full-text indexing so built-in tokenization strategies can be selected per full-text index.

## Functional Scope

- Add a tokenizer option to full-text index metadata and bind it to a known built-in tokenizer implementation.
- Support at least the existing default tokenizer plus one tokenizer with materially different boundaries, such as whitespace-only or ngram/prefix tokenization.
- Ensure indexing, querying, scoring, snippets, rebuilds, and cache keys all use the configured tokenizer consistently.
- Persist and hydrate tokenizer configuration with versioned metadata.
- Reject unknown tokenizer names and invalid tokenizer options before index creation.

## Non-Goals

- Do not load arbitrary external code or runtime plugins.
- Do not make tokenizers mutable after index creation without an explicit rebuild path.

## Acceptance Criteria

- Different tokenizers produce expected token streams and search results for the same documents.
- Restart and rebuild preserve tokenizer-specific index contents and search behavior.
- Unknown tokenizer names/options return clear parser/binder errors.
- Existing full-text indexes continue using the default tokenizer.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering tokenizer selection, invalid tokenizer rejection, restart hydration, rebuild, snippet consistency, and cache separation by tokenizer.
- Include unit tests for tokenizer output and integration tests for search behavior.

## Closeout Steps

- Run the validation commands below.
- Run `cargo build --locked`.
- Run `cargo fmt --all -- --check`.
- Document supported tokenizer names and options.

## Validation

- `cargo test --test integration_sql --quiet`
- `cargo test --test executor --quiet`
- `cntryl-tools validate-tests -f tests/integration_sql.rs`
