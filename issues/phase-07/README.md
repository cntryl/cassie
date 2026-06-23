# Phase 07: Advanced Query And Distributed Backlog

Phase 07 is closed for the current V1 advanced-query scope.

The shipped scope covers the advanced planner, executor, storage-mode, and offline consistency features that depend on the archived Phase 04, Phase 05, and Phase 06 contract surfaces:

1. operator selection feedback with bounded persistence, hydration, planner usage, EXPLAIN diagnostics, and metrics
2. explicit `column_store` table mode behind `CASSIE_EXPERIMENTAL_COLUMN_STORE_ENABLED`, including CRUD/read behavior, storage metadata, and deterministic unsupported schema rewrites
3. merge join planning and execution for proven sorted equi-join shapes
4. vectorized inner and left equi-join execution behind `CASSIE_VECTORIZED_JOINS_ENABLED`, with batch-size controls and observable fallback
5. adaptive execution-plan selection among prevalidated alternatives behind `CASSIE_ADAPTIVE_EXECUTION_ENABLED`
6. runtime operator switching for prevalidated pairs behind `CASSIE_OPERATOR_SWITCHING_ENABLED`
7. multi-instance consistency manifests and offline comparison reports through SQL, REST, catalog, and restart hydration surfaces

These capabilities remain documented as experimental where `docs/feature-support.md` says experimental.
Closing Phase 07 does not promote them to stable or production-ready by itself; it means the planned Phase 07 surface is implemented, tested, documented, and archived for the current scope.

## Archived Contract Dependencies

- Phase 04 runtime-boundary and read access-path contracts: `docs/performance-contracts.md`, `issues/phase-04/README.md`
- Phase 05 write/layout and diagnostics contracts: `docs/performance-contracts.md`, `issues/phase-05/README.md`
- Phase 06 read/access-path diagnostics and proof surface: `issues/phase-06/README.md`

## Explicit Non-Goals

- No advanced operator bypasses the Phase 04 blocking-boundary rules.
- No advanced storage mode introduces a second storage abstraction above Midge.
- No adaptive plan changes SQL-visible semantics, ordering, freshness, timeout, or error behavior.
- No consistency workflow enters the query path or implies replication, quorum reads, or repair.

## Follow-On Work

Future work beyond the closed Phase 07 scope should be tracked as new issues only when a roadmap concept remains active and cannot be satisfied by the archived surfaces above.
