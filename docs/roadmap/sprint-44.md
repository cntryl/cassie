# Sprint 44 - Tier 4 Client-Facing Protocol Benchmarks

Previous: [Sprint 43 - Tier 3 End-to-End Workload Benchmarks](sprint-43.md)
Next: [Roadmap README](README.md)

## Goal

Measure Cassie the way real clients consume it over PostgreSQL wire and HTTP, and make the benchmark suite repeatable in CI.

## Requirements

- Add protocol benchmarks for equivalent SQL, search, vector, and hybrid workloads over PostgreSQL wire and HTTP.
- Measure connection setup, simple queries, prepared statements, result-set size, and concurrent clients for both protocols.
- Add protocol comparison reporting so serialization cost and protocol overhead can be compared directly.
- Wire the benchmark suite into CI as a repeatable artifact-producing job.

## Acceptance Criteria

- Equivalent workloads can be executed over both PostgreSQL wire and HTTP and compared side by side.
- The CI job produces machine-readable benchmark artifacts with the standard result contract.
- The benchmark documentation explains how to run the suite locally and how to interpret the output.

## Tests

- Integration tests for the benchmark job configuration and artifact output.
- Regression tests for protocol comparison report generation.

## Exit Gate

This sprint is complete when tier-4 benchmarks run end to end, the protocol comparison output is useful and repeatable, and the benchmark job is integrated into CI.
