# Query Promotion Evidence

This inventory maps Cassie's query families to an independently selected correctness baseline and
the evidence required before an Experimental surface can be promoted. It prevents a benchmark,
an `EXPLAIN` assertion, or two implementations sharing the same derived state from being treated
as differential correctness evidence.

The canonical support status remains [Feature Support](feature-support.md). Existing focused
corruption, cancellation, and resource-limit tests remain authoritative; this inventory does not
require duplicating them in a generic harness.

## Evidence Rules

- Seeded cases record an explicit `u64` seed and bounded row count.
- The optimized and baseline paths receive the same logical rows in different insertion orders.
- A baseline must avoid the optimized derived state under test. An `EXPLAIN` label alone is not
  proof that it did so.
- Compare exact rows, value types, ordering, errors, transaction visibility, and fallback
  diagnostics. Approximate retrieval additionally compares recall against exact top-k.
- Pagination evidence concatenates every bounded page and compares it with the corresponding
  bounded full result. Empty results and ties are explicit cases.
- Controlled failures must leave no partial result, published success metric, worker permit, or
  accounted-memory reservation.

## Access-Path Inventory

| Family | Status | Optimized path | Independent baseline | Current deterministic evidence | Remaining promotion gap / owner |
| --- | --- | --- | --- | --- | --- |
| Scalar indexes | Experimental | Primary, secondary, composite, covering, partial, or expression index | Same logical rows in a table without the index | `planner_controls::query_promotion_evidence`; scalar cancellation and memory checks in `query_scan_controls` | #11 owns seeded boundary values, insertion-order permutation, page concatenation, predicate-equivalent queries, and transaction overlays. |
| Full-text search | Stable | Persisted posting-block candidates with exact BM25 scoring | Labelled authoritative-row fallback | `search::fulltext_persisted_sql`, `fulltext_retrieval_corruption`, and full-text controls in `query_scan_controls` cover exact results, overlays, fallback, cancellation, and memory | No #11 promotion gap. Retain the focused evidence; do not duplicate it in the shared harness. |
| Exact vector search | Stable | Streaming bounded exact top-k | Exhaustive deterministic fixture ranking | `vector_embeddings::vector_query_stability` and exact-vector controls in `query_scan_controls` | No #11 promotion gap. |
| HNSW | Stable | Persisted graph candidates plus exact source-row reranking | Exact vector top-k over the same source rows | `vector_embeddings::hnsw_indexes`, `vector_query_stability`, and `query_scan_controls::vector_ann_concurrency` | Production-profile evidence is #24, not a cross-family correctness gap. |
| IVFFlat | Stable | Persisted probed-list candidates plus exact source-row reranking | Exact vector top-k over the same source rows | `vector_embeddings::ivfflat_indexes`, `ivfflat_completeness`, `vector_query_stability`, and ANN controls | Production-profile evidence is #25, not a cross-family correctness gap. |
| Hybrid retrieval | Stable | Persisted text/vector/structured candidate intersection and exact final scoring | Explicit exact fallback over authoritative source rows | `vector_embeddings::integration_sql_hybrid_query` plus retrieval-family controls in `query_scan_controls` | No #11 promotion gap. |
| Time-series access | Experimental | Partition/bucket membership and ordered range reads | Authoritative row scan with the same predicates and ordering | `domain_models::time_series_indexes` and `time_series_index_completeness` cover selection, mutation/restart, corruption, fallback, and bounded reads | #27 owns the representative seeded bucket-width/range/pagination matrix; #23 owns dependent rollup and retention workflows. |
| Column-batch analytics | Stable | CBM2 summaries, encoded scans, late materialization, and filtered aggregates | Atomic authoritative-row fallback | `analytics::column_batch_*` and analytical controls in `query_scan_controls` cover exact values, nulls, pruning, corruption, mutation/restart, cancellation, and memory | No #11 promotion gap. Projection lifecycle remains separately Experimental under #28. |
| Graph traversal | Experimental | Persisted adjacency expansion and shortest paths | Native authoritative-edge scan/traversal | `domain_models::integration_sql_graph` and `query_scan_controls::graph_resilience` cover results, overlays, restart/rebuild, ordering, bounded reads, cancellation, and memory | #11 owns a bounded seeded topology permutation comparing native and adjacency results, including ties, disconnected vertices, and overlay visibility. |

## Relational and Execution Inventory

| Surface | Status | Baseline / control | Current deterministic evidence | Remaining promotion gap / owner |
| --- | --- | --- | --- | --- |
| Joins | Experimental | Fixed legal join shape over the same inputs | Planner and executor suites cover supported join kinds, legality, exact results, cancellation, and memory | #11 owns seeded input-order and predicate-equivalent cases for the supported join subset selected for promotion. Adaptive switching remains #12. |
| Subqueries and CTEs | Experimental | Equivalent supported join, aggregate, or direct-query form where semantics permit | SQL and planner suites cover correlated, lateral, non-recursive, and recursive cases | #11 owns an explicit equivalence case only where an independent rewrite exists; unsupported rewrites remain out of scope. |
| Window functions | Experimental | Independently grouped/ordered expected rows for the documented bounded frame | SQL/executor suites cover supported ranking, offset, value, and row-frame behavior | #11 owns seeded ties, null ordering, and input-order permutation for the selected promotion subset. |
| DML and transactions | Experimental | Pre/post authoritative reads from isolated sessions | Catalog, SQL, and transaction suites cover commit, rollback, savepoints, conflicts, and read-your-writes | Cross-family overlay cases are evidence inputs, not a blanket DML promotion. Any remaining DML promotion is separately scoped. |
| Projection lifecycle | Experimental | Authoritative source query plus version/checkpoint state | Analytics projection, replay, comparison, and repair suites | #28 owns lifecycle promotion; stable CBM2 is not reopened by #11. |
| Cost and adaptive planning | Experimental | Fixed-plan execution with adaptive controls disabled | Planner/metrics suites cover decisions, stale feedback, guards, and cleanup | #12 follows #11 and owns representative fixed/adaptive equivalence and rollback evidence. |
| Pull execution, controls, and parallelism | Experimental | Single-worker bounded execution of the same plan where supported | Executor and `query_scan_controls` suites cover early stop, deterministic merge, deadlines, cancellation, workers, and memory | Add a #11 case only when the selected family lacks exact cross-mode result evidence; do not duplicate resource-control tests. |

## Current Shared Fixture

`tests/support/query_evidence.rs` owns the bounded scalar-index differential fixture. Its explicit
seed generates nulls, zero, negatives, duplicates, and ties. The row and indexed stores receive
opposite insertion orders; a transaction overlay is applied to both; every three-row page is
concatenated and compared with the ordered full result; and an empty-result predicate is checked.

Reusable generators, environment guards, and assertions belong under `tests/support/`. Family
tests remain in their existing flat subsystem suite so Cargo integration-test discovery and
ownership stay clear.
