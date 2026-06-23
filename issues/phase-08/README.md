# Phase 08: README Goal Closure

Phase 08 is the active execution gate for closing the gaps identified in `docs/read-model-gap-analysis.md` against the product goals in `README.md`.

This phase consumes the archived Phase 04 through Phase 07 contract surfaces.
It must not reopen those phases or add distributed SQL semantics.

## Core Rule

Phase 08 work makes Cassie a stronger single-node read-model database for event-sourced systems.
Operational-scale features are local metadata, diagnostics, admin workflows, and external-orchestration hooks only.
They must not introduce distributed query execution, consensus, replication, quorum reads, cross-node planning, or automatic remote repair.

## Phase Sequence

Closed baseline:

- Operational scale ownership model: local assignment metadata, restart hydration, catalog diagnostics, and external-routing docs.
- Snapshot and restore: local Midge-directory snapshots, Cassie compatibility manifests, restore validation, and query-after-restore smoke coverage.
- Performance benchmark feedback: manifest-owned 10k/100k Criterion scenarios for core read, replay, rebuild, verification, search/vector/hybrid, pgwire, and HTTP workloads.
- Repair workflow design: deterministic admin dry-run plans for all repair scopes, local row/range hash repair with audit records, post-repair verification, and explicit no-automatic/no-distributed repair boundaries.
- Read optimization follow-on: Phase 06 read paths rebaselined against product docs, tenant filtered pages locked with composite scalar range coverage, and mixed-direction/expression-index depth left as explicit follow-on scope.
- Time-series completion: row-backed timestamp range scans, bucket diagnostics, mutation/restart correctness, retention freshness effects, rollup refresh behavior, and manual 10k/100k feedback benches.
- Client compatibility matrix: read-model-focused client status for tokio-postgres, psql, sqlx, diesel, prisma, SQLAlchemy, migration tools, default tokio-postgres coverage, and opt-in psql validation.
- Procedure non-goal resolution: limited experimental compatibility/admin procedures remain available, while stored-procedure platforms, triggers, procedural languages, and OLTP business logic stay out of scope.
- Production-ready classification: feature-family readiness matrix with owners, support levels, evidence, benchmark/operational signals, restart coverage, and blockers.
- Capacity management and docs reconciliation: advisory sizing signals, operator thresholds, cache/index/rebuild/fallback guidance, manual benchmark workflow, and stale-doc cleanup.

Remaining sequence:

- None.

## Required Gates

- Phase 04 runtime and access-path contracts are archived in `docs/performance-contracts.md` and `issues/phase-04/README.md`.
- Phase 05 write-path contracts are archived in `docs/performance-contracts.md` and `issues/phase-05/README.md`.
- Phase 06 read optimization contracts are archived in `issues/phase-06/README.md`.
- Phase 07 advanced query contracts are archived in `issues/phase-07/README.md`.

## Non-Goals

- No distributed SQL execution.
- No cross-node query planning.
- No replication, quorum reads, or consensus.
- No automatic repair in the query path.
- No second storage abstraction above Midge.
