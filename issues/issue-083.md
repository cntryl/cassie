# Issue 083: Adaptive Candidate Sizing

Milestone: V2 - Query Performance
Area: Adaptive
Status: Open
Priority: P1

## Requirement

Adapt full-text, vector, and hybrid candidate limits so filtered top-k queries return enough correct rows without overscanning by default.

## Functional Scope

- Derive an initial candidate budget from LIMIT, OFFSET, metadata filters, and available cardinality/runtime feedback.
- Expand candidate batches deterministically when post-filtering leaves too few rows, until enough rows are produced or the source is exhausted.
- Apply hard minimum/maximum candidate bounds from runtime limits to prevent unbounded work.
- Preserve exact final ordering, tie-breaking, LIMIT, and OFFSET semantics.
- Report initial budget, expansions, final candidate count, and exhaustion through metrics/EXPLAIN.

## Non-Goals

- Do not approximate final results; correctness remains exact for supported query shapes.
- Do not introduce background training or workload-specific tuning outside captured runtime feedback.

## Acceptance Criteria

- Top-k search/vector/hybrid queries with selective filters return the same rows as an exhaustive baseline.
- Candidate expansion stops once LIMIT/OFFSET requirements are satisfied or all candidates are exhausted.
- Runtime limits cap expansion and return a clear query-limit error rather than partial silent results.
- Metrics make adaptive sizing decisions observable.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering selective filters, OFFSET expansion, exhausted candidates, limit cap error, and runtime-feedback-informed initial sizing.
- Include at least one regression test for deterministic tie order.

## Closeout Steps

- Run the validation commands below.
- Run `cargo build --locked` because this touches search/vector execution controls.
- Run `cargo fmt --all -- --check`.
- Document any new runtime limits or metrics.

## Validation

- `cargo test --test metrics --quiet`
- `cargo test --test planner --quiet`
- `cntryl-tools validate-tests -f tests/metrics.rs`
