# Sprint 41 - Tier 1 Hot Path Benchmarks

Previous: [Sprint 40 - Benchmark Harness and Output Contract](sprint-40.md)
Next: [Sprint 42 - Tier 2 Subsystem Benchmarks](sprint-42.md)

## Goal

Implement the first tier of benchmarks by isolating the smallest critical operations in the query engine.

## Requirements

- Add benchmarks for row encode/decode, key encode/decode, field lookup, predicate evaluation, batch filter, batch projection, and value comparison.
- Add benchmarks for tokenization and ranking primitives, including BM25 scoring and cosine, dot-product, and L2 distance.
- Add coverage for query parameter binding and row-to-Postgres-wire plus row-to-JSON encoding.
- Keep each benchmark focused on one operation so setup cost does not dominate the measurement.

## Acceptance Criteria

- Each tier-1 benchmark reports the required metrics in JSON.
- Microbenchmarks are repeatable enough for local development and CI usage.
- The benchmark harness can run the full tier-1 suite from one command.

## Tests

- Harness tests for benchmark registration and result aggregation.
- Regression tests for the expected metric fields for each tier-1 case.

## Exit Gate

This sprint is complete when the tier-1 suite executes cleanly, the results match the JSON contract, and the benchmark build and validation gates are green.
