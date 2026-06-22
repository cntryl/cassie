# Phase 07: Advanced Query And Distributed Backlog

Phase 07 is the parked advanced backlog.

These issues are intentionally after the foundation, write-path, and read-path implementation contracts in phases 04 through 06.
They add adaptive planning, advanced operators, column-store tables, runtime switching, and multi-instance comparison only after Cassie can prove the simpler runtime and access-path contracts.

Phase 05 write optimization is closed; its archived contract and diagnostic surface lives in `docs/performance-contracts.md` and `issues/phase-05/README.md`.

## Core Rule

Advanced execution work must not become a shortcut around the lower-level contracts.
Each phase 07 issue must consume the archived phase 04 runtime-boundary and read-access-path contract surfaces, the archived phase 05 write/layout and diagnostics contract surface, and phase 06 read/access-path diagnostics before adding new planner or executor behavior.

## Phase Sequence

1. Operator selection feedback: feed bounded operator observations back into future plans.
2. Full column-store tables: promote columnar storage to an explicit table mode only after write/layout and read-shape contracts are stable.
3. Merge joins: add a sorted-input join alternative with explicit ordering proof.
4. Vectorized joins: add batch join kernels with bounded row/batch conversion rules.
5. Adaptive execution plans: pre-plan safe alternatives and choose only at explicit decision points.
6. Runtime operator switching: switch only among pre-validated operators with state-transfer rules.
7. Multi-instance consistency checks: compare manifests offline without adding distributed query semantics.

## Required Gates

- The archived phase 04 runtime-boundary contract surface must be complete before runtime-adaptive or async-surfaced Phase 07 work begins.
- The archived phase 05 write-layout contract surface must be complete before Phase 07 changes persistent key or storage layout.
- Phase 06 issue 05 must be complete before Phase 07 adds new planner/executor alternatives that require EXPLAIN or metrics assertions.
- The archived phase 04 read access-path contract surface must be complete before Phase 07 adds any new access path or storage mode.

## Non-Goals

- No advanced operator may bypass Phase 04 blocking-boundary rules.
- No advanced storage mode may introduce a second storage abstraction above Midge.
- No adaptive plan may change SQL-visible semantics, ordering, freshness, timeout, or error behavior.
- No distributed consistency check may enter the query path or imply replication, quorum reads, or repair.
