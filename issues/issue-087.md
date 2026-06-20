# Issue 087: Buffer Reuse

Milestone: V2 - Query Performance
Area: Execution
Status: Open
Priority: P1

## Requirement

Reuse bounded per-query scratch buffers in executor hot paths to reduce allocation churn without leaking data across rows, queries, sessions, or tenants.

## Functional Scope

- Add reusable buffers for row decoding, projection construction, expression evaluation, sorting/top-k, aggregate state updates, and vector/search scoring where ownership is local.
- Bound buffer capacity by runtime limits and shrink/clear buffers after large queries.
- Ensure reused buffers are reset between rows and are not shared across concurrent queries without synchronization.
- Preserve deterministic output ordering and exact error behavior.
- Expose enough metrics or benchmark instrumentation to verify reuse is active and bounded.

## Non-Goals

- Do not add a global unbounded buffer pool.
- Do not retain user row contents longer than the query/session requires.

## Acceptance Criteria

- Repeated execution of projected/filter/sort queries performs fewer allocations or buffer creations than the baseline.
- Query results remain identical for multi-batch execution, errors, and concurrent sessions.
- Large temporary buffers are released or shrunk according to runtime limits.
- Tests cover clearing behavior so previous row values cannot appear in later results.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering multi-batch projected scans, sort/top-k reuse, aggregate reuse, error cleanup, and concurrent query isolation if concurrency primitives are involved.
- Include instrumentation or benchmark evidence for fewer allocations.

## Closeout Steps

- Run the validation commands below.
- Run `cargo build --locked` and targeted executor benchmarks if allocation instrumentation depends on them.
- Run `cargo fmt --all -- --check`.
- Document any new runtime limit or metric names.

## Validation

- `cargo test --test planner --quiet`
- `cargo test --test executor --quiet`
- `cntryl-tools validate-tests -f tests/planner.rs`
- `cntryl-tools validate-tests -f tests/executor.rs`
