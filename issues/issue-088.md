# Issue 088: Allocation Reduction

Milestone: V2 - Query Performance
Area: Execution
Status: Open
Priority: P1

## Requirement

Reduce avoidable heap allocations in the planner and executor hot paths while keeping public behavior and storage formats unchanged.

## Functional Scope

- Identify allocation hot spots in parsing/planning/execution using existing benchmarks, allocation counters, or targeted instrumentation.
- Remove unnecessary string/value clones, temporary vectors, repeated field-name normalization, and per-row allocations in projected scans, filters, sort/top-k, aggregates, and search/vector scoring.
- Reuse existing internal APIs where possible before adding new abstractions.
- Preserve deterministic query plans, diagnostics, error text, result order, and protocol output.
- Add regression coverage so future changes do not reintroduce the largest allocation sources addressed by this issue.

## Non-Goals

- Do not rewrite the executor architecture wholesale.
- Do not trade correctness or maintainability for micro-optimizations without measured evidence.

## Acceptance Criteria

- At least one targeted benchmark or allocation-count test shows a measurable reduction in allocations for a representative hot path.
- Existing planner/executor integration tests continue to pass with identical user-visible results.
- The implementation includes comments only where lifetime or reuse constraints are not obvious.
- No new unbounded caches or long-lived user-data retention are introduced.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` for the optimized behavior when user-visible risk exists.
- Add a focused allocation/benchmark gate for the optimized hot path, or record before/after benchmark evidence in closeout if deterministic allocation tests are not practical.

## Closeout Steps

- Run the validation commands below.
- Run `cargo build --locked`.
- Run `cargo fmt --all -- --check`.
- Record the measured allocation reduction and the benchmark/test command used.

## Validation

- `cargo test --test planner --quiet`
- `cargo test --test executor --quiet`
- `cntryl-tools validate-tests -f tests/planner.rs`
- `cntryl-tools validate-tests -f tests/executor.rs`
