# Issue 082: Runtime Feedback

Milestone: V2 - Query Performance
Area: Adaptive
Status: Open
Priority: P1

## Requirement

Record execution feedback for normalized plans and physical operators so later planning can compare estimates with observed work.

## Functional Scope

- Capture actual rows in/out, elapsed time, storage reads/writes, temp writes, candidate counts, and error status per operator where instrumentation already exists or can be added locally.
- Key feedback by normalized SQL fingerprint, database, collection, schema epoch, and operator kind without storing bind values or sensitive literals.
- Keep feedback bounded by count and age, with deterministic eviction and no unbounded growth.
- Expose feedback through metrics and EXPLAIN ANALYZE diagnostics.
- Make feedback advisory only: failures to read/write feedback cannot fail user queries.

## Non-Goals

- Do not implement adaptive execution or runtime operator switching here.
- Do not persist personally sensitive bind values or raw SQL literals in feedback records.

## Acceptance Criteria

- Repeated execution of the same normalized plan accumulates feedback counters without sharing bind values.
- EXPLAIN ANALYZE reports actual operator information that matches the executed query.
- Feedback records are invalidated or partitioned across schema/catalog changes, database names, and collection names.
- Bounded retention and eviction are covered by deterministic tests.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering feedback capture, repeated-query aggregation, schema invalidation, retention eviction, and EXPLAIN ANALYZE output.
- Include metrics assertions for feedback hits, misses, writes, and evictions.

## Closeout Steps

- Run the validation commands below.
- Run `cargo build --locked` because this touches runtime diagnostics.
- Run `cargo fmt --all -- --check`.
- Update metrics documentation if new fields are added.

## Validation

- `cargo test --test metrics --quiet`
- `cargo test --test planner --quiet`
- `cntryl-tools validate-tests -f tests/metrics.rs`
