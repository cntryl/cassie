# Issue 107: Parallel Scoring

Milestone: V3 - Advanced Query Features
Area: Execution
Status: Open
Priority: P2

## Requirement

Score full-text, vector, and hybrid candidates across bounded parallel workers while preserving exact final ranking and error behavior.

## Functional Scope

- Partition candidate sets deterministically for BM25, vector distance, and hybrid scoring.
- Share read-only scoring metadata safely across workers and aggregate partial top-k results with stable tie-breaking.
- Respect query timeout, result limit, candidate limit, and cancellation across all workers.
- Keep single-worker fallback for small candidate sets, unsupported score expressions, or worker limit of one.
- Report workers, candidate partitions, scored rows, and fallback reason through EXPLAIN/metrics.

## Non-Goals

- Do not approximate scores or change BM25/vector/hybrid formulas.
- Do not parallelize external embedding provider calls in this issue.

## Acceptance Criteria

- Parallel scoring returns identical scores, rows, and deterministic tie order as single-worker scoring.
- Worker failures, timeout, and cancellation are reported once and clean up all worker state.
- Metrics show candidate counts and worker participation.
- Unsupported query shapes fall back without changing results.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering full-text, vector, hybrid scoring, stable tie order, timeout cleanup, fallback, and worker-limit behavior.
- Include executor and metrics assertions.

## Closeout Steps

- Run the validation commands below.
- Run `cargo build --locked`.
- Run `cargo fmt --all -- --check`.
- Add benchmark evidence for large candidate sets.

## Validation

- `cargo test --test planner --quiet`
- `cargo test --test executor --quiet`
- `cntryl-tools validate-tests -f tests/planner.rs`
- `cntryl-tools validate-tests -f tests/executor.rs`
