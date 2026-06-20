# Issue 102: Expression Indexes

Milestone: V3 - Advanced Query Features
Area: Indexes
Status: Open
Priority: P2

## Requirement

Support indexes over deterministic scalar expressions so queries such as `WHERE lower(email) = 'x'` can use index access.

## Functional Scope

- Parse and bind `CREATE INDEX name ON table (<expression>)` for expression items that are not plain field identifiers.
- Allow only immutable built-in scalar functions, casts, field references, literals, and deterministic operators supported by the binder.
- Persist and hydrate a versioned normalized expression representation in index metadata.
- Compute expression keys on ingest, SQL writes, updates, rebuild, restart hydration, rename, and drop.
- Match query predicates to expression indexes by normalized expression equivalence and compatible comparison operators.

## Non-Goals

- Do not support user-defined functions, volatile context functions, subqueries, aggregates, window functions, or expressions depending on non-row state.
- Do not change scalar function semantics or collation behavior beyond existing Cassie behavior.

## Acceptance Criteria

- Expression indexes are parsed, bound, persisted, hydrated, maintained, rebuilt, and dropped.
- Planner uses expression indexes for equivalent deterministic predicates and falls back for non-equivalent or unsupported expressions.
- Null, missing-field, cast failure, and function error behavior matches non-indexed expression evaluation.
- EXPLAIN identifies the selected expression index and expression key.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering `lower(email)`, casts, invalid functions, write/update maintenance, restart hydration, rebuild, expression-equivalence matching, and fallback.
- Include parser, planner, and SQL integration tests.

## Closeout Steps

- Run the validation commands below.
- Run `cargo build --locked` because this touches AST/binder/catalog/planner/executor.
- Run `cargo fmt --all -- --check`.
- Update index roadmap docs if supported expression classes change.

## Validation

- `cargo test --test parser --quiet`
- `cargo test --test planner --quiet`
- `cargo test --test integration_sql --quiet`
- `cntryl-tools validate-tests -f tests/parser.rs`
- `cntryl-tools validate-tests -f tests/planner.rs`
