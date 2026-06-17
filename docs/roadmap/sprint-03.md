# Sprint 03 - SQL Parser and Binder V1

Previous: [Sprint 02 - Midge Storage Contract and Catalog Hydration](completed/sprint-02.md)  
Next: [Sprint 04 - Planner, Optimizer, and Physical Plan Determinism](sprint-04.md)

## Goal

Finish Cassie's custom SQL parser and binder for the V1 query surface without introducing a third-party SQL parser. The result should be deterministic enough for REST, PostgreSQL wire simple query, and PostgreSQL wire extended query paths to share.

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

- Support V1 `SELECT`, `FROM`, `WHERE`, `ORDER BY`, `LIMIT`, and `OFFSET`.
- Support projection aliases and order-by aliases.
- Support parameter references like `$1`, `$2`, and bind-time parameter validation.
- Support logical operators, comparison operators, `LIKE`, parentheses, and deterministic boolean precedence.
- Support Cassie functions through a registry: `search`, `search_score`, `vector_distance`, `vector_score`, `cosine_distance`, `dot_product`, `hybrid_score`, and snippet behavior.
- Support pgvector-style order operators: `<=>`, `<->`, and `<#>`.
- Binder validates collection existence, function existence, function arity, and feasible column/function references.
- Negative or malformed pagination fails deterministically.
- Unsupported SQL produces a clear parse or planner error.

## Acceptance Criteria

- Parser and binder tests cover supported SQL forms and unsupported syntax.
- Same query text yields the same AST every run.
- Unknown functions and invalid arity fail before execution.
- Malformed `LIMIT` and `OFFSET` clauses fail during parsing.
- Collection validation happens before planning.
- Function names are treated case-insensitively where appropriate.
- `cargo build` passes.
- `cargo clippy --all-targets --all-features -- -D warnings` passes.
- All touched tests pass `cntryl-tools validate-tests`.

## Tests

- `tests/parser.rs`: SELECT with aliases, filters, sorting, pagination, parameters, functions, pgvector operators, and precedence.
- `tests/parser.rs`: unknown function and bad function arity fail during binding.
- `tests/parser.rs`: malformed and negative pagination fails.
- `tests/parser.rs`: case-insensitive function binding works.
- Add tests only as single behaviors and keep parser failures separate from binder failures.

## Exit Gate

This sprint is complete when parser and binder behavior is deterministic, all touched tests are validator-clean, `cargo test --test parser` passes, `cargo build` passes, and Clippy is clean with warnings denied.
