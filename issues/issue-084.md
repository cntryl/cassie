# Issue 084: Covering Indexes

Milestone: V2 - Query Performance
Area: Indexes
Status: Open
Priority: P1

## Requirement

Use index payloads to answer covered projection queries without fetching row blobs when every required field is available from the selected index.

## Functional Scope

- Treat row blobs as the source of truth and covering indexes as acceleration.
- Select a covering index only when filter predicates match index keys and projection/order/filter fields needed after scan are present in key fields or INCLUDE payload fields.
- Return included values directly from the index payload with the same SQL types, null behavior, sparse-field behavior, aliases, and deterministic ordering as row fetches.
- Fall back to row blob fetches when any requested value is not covered, payload version is unsupported, or predicate shape is not covered.
- Surface covered-index scans in EXPLAIN and metrics, including avoided row fetches.

## Non-Goals

- Do not add INCLUDE column syntax or metadata in this issue; that is issue 085.
- Do not use covering indexes for expressions unless issue 102 has provided expression-index payload support.

## Acceptance Criteria

- Covered SELECT queries over scalar/composite indexes return identical rows to row-blob execution and perform zero row fetches for covered fields.
- Queries requesting non-covered columns fall back to row fetches without changing results.
- Covered execution works after restart and index rebuild.
- EXPLAIN identifies the selected covering index and whether the plan is fully covered.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering fully covered projection, non-covered fallback, covered ordering, null/sparse included values, restart hydration, and rebuild.
- Include planner, executor, and integration coverage.

## Closeout Steps

- Run the validation commands below.
- Run `cargo build --locked` because this touches planner/executor/catalog contracts.
- Run `cargo fmt --all -- --check`.
- Update `docs/index_constraint_roadmap.md` if key/payload shape changes.

## Validation

- `cargo test --test parser --quiet`
- `cargo test --test planner --quiet`
- `cargo test --test integration_sql --quiet`
- `cntryl-tools validate-tests -f tests/parser.rs`
- `cntryl-tools validate-tests -f tests/planner.rs`
