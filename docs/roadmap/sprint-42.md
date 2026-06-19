# Sprint 42 - Tier 2 Subsystem Benchmarks

Previous: [Sprint 41 - Tier 1 Hot Path Benchmarks](sprint-41.md)
Next: [Sprint 43 - Tier 3 End-to-End Workload Benchmarks](sprint-43.md)

## Goal

Measure complete query-engine components in isolation so parser, binder, planner, executor, and search/vector subsystems can be compared independently.

## Requirements

- Add benchmarks for SQL lexing, parsing, binding, logical planning, and physical planning.
- Add subsystem benchmarks for simple scans, indexed filters, full-text search, vector brute-force execution, hybrid execution, and projection write paths.
- Add a small number of representative workloads to avoid turning the sprint into a massive matrix of combinations.
- Capture p50, p95, p99, throughput, allocations, and subsystem-specific counters such as rows scanned or candidates scored.

## Acceptance Criteria

- Each subsystem benchmark can run in isolation with a known dataset and produce machine-readable results.
- The results clearly distinguish parser, planner, and executor costs.
- The benchmark suite is scoped enough to run in regular CI without becoming a bottleneck.

## Tests

- Regression tests for result aggregation across subsystem workloads.
- Smoke tests that confirm the benchmark runner can invoke each subsystem benchmark entry point.

## Exit Gate

This sprint is complete when tier-2 benchmarks execute and report stable subsystem metrics, and the harness remains fast enough for repeatable CI use.
