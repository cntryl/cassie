# Phase 10 Issue 04: Search Vector Hybrid And Graph Retrieval Efficiency

## Status

Open.

## Goal

Optimize retrieval bottlenecks for full-text, vector, hybrid, and graph paths ranked by Issue 01 while preserving scoring, ordering, fallback, and graph traversal semantics.

## Dependencies

- `issues/phase-10/issue-01.md` is complete.
- The Phase 10 report assigns retrieval bottlenecks to this issue.

## Implementation Plan

1. Pick only retrieval bottlenecks assigned to this issue by the Phase 10 report.
2. Preserve existing score ordering, tie-breaking, candidate limits, vector metric semantics, hybrid mixing, and graph traversal output shape.
3. Prefer candidate pruning, sidecar locality, bounded reranking, vector allocation reduction, graph adjacency scan reduction, and cache reuse before adding new index formats.
4. Keep fallback counters and EXPLAIN evidence explicit for any optimized path.
5. Update the Phase 10 report with before/after evidence.

## Acceptance Criteria

- Search/vector/hybrid results remain deterministic.
- Graph `graph_neighbors`, `graph_expand`, and `graph_shortest_path` semantics remain unchanged.
- Every optimized path has before/after benchmark evidence.

## Validation

Run in order:

```sh
cargo build --locked --bin cassie
CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo test --locked
cargo fmt --all -- --check
cntryl-tools validate-tests -f <touched-test-file>
cargo bench --locked --bench <touched-benchmark> --no-run
```

Run the actual touched benchmark before close-out.
