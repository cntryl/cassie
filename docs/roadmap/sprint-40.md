# Sprint 40 - Benchmark Harness and Output Contract

Previous: [Sprint 39 - Schema DDL Breadth and Index Variants](sprint-39.md)
Next: [Sprint 41 - Tier 1 Hot Path Benchmarks](sprint-41.md)

## Goal

Create a reusable benchmark harness that can execute a single workload, collect the required metrics, and emit deterministic machine-readable JSON.

## Requirements

- Add a benchmark entry point that can run a named workload against a local Cassie instance or an in-process engine.
- Support workload selection, dataset selection, warmup, iterations, and repeatable output directories.
- Define the output contract for every benchmark record: tier, name, dataset, rows, duration_ms, p50_ms, p95_ms, p99_ms, throughput, allocations, bytes_allocated, cpu_percent, and memory_mb.
- Keep benchmark execution isolated from production paths so local runs and CI jobs share the same harness.
- Document the minimum commands required to run a benchmark and inspect JSON output.

## Acceptance Criteria

- A single benchmark can be launched from the command line and write JSON to disk.
- The JSON schema is stable and accepted by CI parsing logic.
- Benchmark setup is repeatable and does not depend on manual state.

## Tests

- Unit tests for benchmark config parsing and JSON serialization.
- Smoke tests for a minimal benchmark run and output-file creation.
- Documentation examples that match the actual CLI contract.

## Exit Gate

This sprint is complete when the harness can run a sample benchmark end to end, the JSON output contract is validated, and the touched tests and build gates are green.
