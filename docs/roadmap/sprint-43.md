# Sprint 43 - Tier 3 End-to-End Workload Benchmarks

Previous: [Sprint 42 - Tier 2 Subsystem Benchmarks](sprint-42.md)
Next: [Sprint 44 - Tier 4 Client-Facing Protocol Benchmarks](sprint-44.md)

## Goal

Measure Cassie end to end inside a running service boundary using a small set of representative datasets and workload classes.

## Requirements

- Add end-to-end benchmarks for simple SQL queries, indexed filters, range queries, sort-plus-limit, full-text search, vector search, hybrid search, and mixed ingest-plus-query load.
- Support at least three dataset sizes: 10k, 1M, and 10M rows.
- Cover cold-start and warm-start behavior plus a small set of concurrent query scenarios.
- Track end-to-end latency, throughput, ingest rate, rebuild time, memory usage, CPU utilization, and error rate.

## Acceptance Criteria

- The benchmark runner can execute representative end-to-end workloads with one or more dataset sizes.
- The results expose the effect of dataset growth and concurrency on service-level behavior.
- The workload set is broad enough to reflect the main engine paths without becoming unmaintainable.

## Tests

- Dataset fixture generation smoke tests.
- Regression tests for end-to-end benchmark result collection and serialization.

## Exit Gate

This sprint is complete when tier-3 workloads run successfully, produce stable service-level metrics, and can be replayed locally or in CI.
