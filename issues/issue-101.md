# Issue 101: Partial Indexes

Milestone: V3 - Advanced Query Features
Area: Indexes
Status: Open
Priority: P2

## Requirement

Support partial scalar/composite indexes with deterministic predicates so filtered read-model queries can use smaller index ranges.

## Functional Scope

- Parse and bind `CREATE INDEX name ON table USING btree (fields...) WHERE <predicate>`.
- Allow deterministic predicates over fields, literals, casts, comparisons, `AND`, `OR`, `NOT`, `IS NULL`, `IN`, and `BETWEEN` where those expressions are already supported by SQL binding.
- Persist and hydrate the predicate AST or a versioned normalized predicate representation in index metadata.
- Maintain index entries only for rows whose current values satisfy the partial predicate across ingest, SQL writes, updates, deletes, rebuild, rename, drop, and restart.
- Select a partial index only when the query predicate implies the index predicate for supported simple conjunction/equality/range shapes; otherwise use the correctness fallback.

## Non-Goals

- Do not implement arbitrary theorem proving or implication for all SQL expressions.
- Do not support volatile functions, user-defined functions, subqueries, joins, or aggregate expressions in partial predicates.

## Acceptance Criteria

- Partial indexes are parsed, bound, persisted, hydrated, rebuilt, and dropped correctly.
- Write maintenance adds, removes, or updates index entries as rows enter or leave the predicate.
- Planner uses partial indexes only for safe predicate shapes and never returns rows outside the query predicate.
- EXPLAIN identifies selected partial indexes and fallback reasons for unsupported implication.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering syntax, invalid predicates, write membership changes, restart hydration, rebuild, safe planner selection, and unsafe fallback.
- Include parser, planner, and SQL integration tests.

## Closeout Steps

- Run the validation commands below.
- Run `cargo build --locked` because this touches parser/AST/catalog/planner/executor.
- Run `cargo fmt --all -- --check`.
- Update `docs/index_constraint_roadmap.md` if predicate support or non-goals change.

## Validation

- `cargo test --test parser --quiet`
- `cargo test --test planner --quiet`
- `cargo test --test integration_sql --quiet`
- `cntryl-tools validate-tests -f tests/parser.rs`
- `cntryl-tools validate-tests -f tests/planner.rs`
