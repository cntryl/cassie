# Issue 106: Parallel Scans

Milestone: V3 - Advanced Query Features
Area: Execution
Status: Open
Priority: P2

## Requirement

Execute eligible collection/index scans across bounded parallel workers while preserving deterministic query results and resource limits.

## Functional Scope

- Partition row-blob and index scan ranges into deterministic non-overlapping shards.
- Use runtime-configured worker limits and respect query timeout, result limit, temp spill budget, and cancellation.
- Merge shard results with stable row ordering before downstream sort/offset/limit semantics are applied.
- Keep single-threaded fallback for small scans, unsupported storage iterators, tests that require deterministic single-worker execution, or runtime worker limit of one.
- Report parallel scan workers, shards, rows scanned, and fallback reason through EXPLAIN/metrics.

## Non-Goals

- Do not parallelize writes or transaction-like schema/data mutations.
- Do not change final result ordering for queries without explicit ORDER BY beyond current deterministic behavior.

## Acceptance Criteria

- Parallel and single-worker scan modes produce identical rows, errors, and metrics totals for supported queries.
- Timeouts and cancellation stop all workers and leave no background sessions running.
- Worker count is bounded and configurable.
- EXPLAIN shows when a parallel scan is selected.

## Required Tests

- Add `should_` tests with `// Arrange / Act / Assert` covering partitioning, deterministic merge, filters/projections, timeout cleanup, fallback, and worker-limit configuration.
- Include executor and planner tests.

## Closeout Steps

- Run the validation commands below.
- Run `cargo build --locked`.
- Run `cargo fmt --all -- --check`.
- Add benchmark evidence for large scans.

## Validation

- `cargo test --test planner --quiet`
- `cargo test --test executor --quiet`
- `cntryl-tools validate-tests -f tests/planner.rs`
- `cntryl-tools validate-tests -f tests/executor.rs`
