# Issue 086: Zero-Copy Value Access

Milestone: V2 - Query Performance
Area: Execution
Status: Open
Priority: P1

## Requirement

Allow scan, filter, projection, sort, and aggregate paths to read row values through borrowed or lazily decoded accessors instead of eagerly cloning every `Value`.

## Functional Scope

- Introduce an internal value-access API that can borrow from row blobs or batch buffers while preserving existing owned `Value` results at public boundaries.
- Apply zero-copy access to filters, projection pruning, order-by evaluation, aggregate input reads, and search/vector metadata lookups where lifetimes are local to execution.
- Keep row blob decoding version-aware and compatible with sparse rows, retired field IDs, arrays, UUIDs, temporal values, JSON, vectors, and missing fields.
- Prevent borrowed values from escaping query execution or crossing async/task boundaries unsafely.
- Add metrics or benchmark assertions showing reduced clones/decodes on targeted hot paths.

## Non-Goals

- Do not change public Rust API result ownership, pgwire encoding, REST JSON responses, or stored row encoding.
- Do not introduce unsafe code unless it is narrowly justified and covered by tests.

## Acceptance Criteria

- Query results match existing owned-value execution for filters, projections, sorts, aggregates, and sparse rows.
- Tests prove borrowed/lazy access does not read stale data after buffer reuse.
- Hot-path clone/decode counts or benchmark allocation counts are lower for projected filtered scans.
- Fallback to owned values remains available for complex expressions that require ownership.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering projected scans, filtered scans, order-by on unprojected fields, sparse/missing fields, and complex values such as arrays/vectors/JSON.
- Include focused unit tests near row/batch accessors and executor integration tests.

## Closeout Steps

- Run the validation commands below.
- Run `cargo build --locked` and `cargo test --locked` because this touches shared executor internals.
- Run `cargo fmt --all -- --check`.
- Record benchmark or instrumentation evidence in the issue closeout note.

## Validation

- `cargo test --test planner --quiet`
- `cargo test --test executor --quiet`
- `cntryl-tools validate-tests -f tests/planner.rs`
- `cntryl-tools validate-tests -f tests/executor.rs`
