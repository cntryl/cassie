# Phase 06: Read Optimization

Phase 06 is closed for the current V1 read-optimization scope.

The shipped scope now covers the read paths Cassie can prove and execute end to end:

- row-id point-lookup planning and execution diagnostics
- scalar secondary `index_seek` reads for proven equality lookups
- composite equality `prefix_scan` reads for proven secondary-key prefixes
- scalar `range_scan` reads for proven bounded field predicates
- scalar `ordered_bounded_scan` reads for proven index-order `ORDER BY ... LIMIT` shapes
- limit-pushdown and bounded projected scans where the current planner can prove them
- row-id ordered-page execution with `storage_top_k`, `keyset`, and degraded `offset` paths
- explicit `access_path`, `fallback_reason`, `pagination_strategy`, `top_k_mode`, `early_stop`, and `projection_shape` EXPLAIN labels
- materialized projection freshness/output diagnostics and runtime-join degradation labels
- runtime read-path metrics and tier-2 benchmark ownership for row-id and scalar ordered-read paths

Remaining follow-on scope after this close-out:

- broader filtered-page lowering that requires residual predicate proof
- descending or mixed-direction multi-column secondary ordering beyond the current same-direction proof
- richer expression-index lowering beyond the current conservative metadata selection

Phase 06 optimizes Cassie's read side for read-model workloads.

The goal is not generic SQL cleverness.
The goal is to make supported read-model query shapes lower into Midge-native access paths with explicit planner proof, executor behavior, diagnostics, and benchmarks.

SQL and pgwire are read interfaces.
They must not force Cassie into collection scans, broad materialization, late sorting, or offset discard work when a read-model query pattern has an expected bounded storage path.

## Core Rule

A read pattern is not supported because it returns correct rows.
A read pattern is supported only when it returns correct rows through the intended Midge-efficient access path, or when the read model is explicitly shaped/materialized for that access path.

Each phase 06 change must define:

- required access-path behavior
- forbidden access-path behavior
- planner or EXPLAIN assertions
- executor behavior and fallback semantics
- benchmark evidence or compile-validated benchmark ownership

Each issue should include a concrete `Implementation Plan` section with expected files/modules, TDD order, benchmark updates, diagnostics, and close-out sequence.
The goal is that implementation work is mostly mechanical once the issue is picked up.

## Read Pattern Categories

| Pattern | Purpose | Expected path |
| --- | --- | --- |
| Primary lookup | Detail reads by row id | point lookup or unique index lookup |
| Secondary lookup | Tenant/external id lookup | composite index seek |
| Range scan | Timelines and audit reads | bounded range or prefix scan |
| Ordered page | List/dashboard page | order-compatible bounded scan or top-K path |
| Filtered page | Work queues and scoped lists | composite predicate/order path or materialized shape |
| Count/exists | Badges, guards, presence checks | short-circuit or accelerated scalar path |
| Full-text search | Keyword retrieval | full-text candidate path plus exact final scoring |
| Vector search | Nearest-neighbor retrieval | vector index or explicit brute-force fallback |
| Hybrid search | Text + vector retrieval | bounded candidates plus exact final scoring |
| Time bucket / aggregate | Dashboard summaries | aggregate acceleration, rollup, column batch, or materialized summary |
| Projection-shaped read | Product-critical multi-entity view | pre-shaped projection instead of runtime-heavy join |

## Phase Sequence

Phase 06 consumes the archived phase 04 read access-path contract surface in `docs/performance-contracts.md` and `issues/phase-04/README.md` for access-path contracts.
It does not redefine read-shape vocabulary while implementing read-path behavior.

1. Predicate/order/limit pushdown: lower eligible reads into bounded storage scans.
2. Keyset pagination: make interactive paging seek-based where the query shape proves ordering.
3. Top-K and early stop execution: stop scanning/scoring once exactness allows.
4. Projection-shaped read layouts: require materialization when generic execution cannot be efficient.
5. Access-path assertions and diagnostics: make optimized and degraded paths visible and testable.

## Non-Goals

- No second storage abstraction.
- No approximate answers for exact SQL paths.
- No hidden projection rewrites without EXPLAIN/diagnostic visibility.
- No misleading `EXPLAIN` labels when the planner cannot prove an access path.
- No optimization that changes ordering, null, offset, limit, or error semantics.
- No read implementation should infer an access path that is absent from the archived phase 04 read access-path contract surface.
