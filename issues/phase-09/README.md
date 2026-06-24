# Phase 09: Production Depth And Operational Orchestration

Phase 09 is the active follow-on gate after Phase 08 README-goal closure.
It tracks planned or planned-by-depth roadmap work that remains after the MVP baseline: external orchestration contracts, production evidence, read-path depth, client probes, capacity diagnostics, repair depth, adaptive planning depth, experimental promotion criteria, and large-file extraction.

This phase must preserve the Phase 08 boundary: Cassie exposes local metadata, diagnostics, and admin workflows for independent read-model nodes, but it does not become distributed SQL.

## Core Rule

Phase 09 work strengthens production trust and depth without broadening Cassie into a general-purpose database.
Every implementation slice must remain tied to read-model workloads, Midge-native storage behavior, explicit fallback semantics, and production-readiness evidence.

## Phase Sequence

Closed baseline:

- Issue 07 pgwire client probe expansion: SQLAlchemy Core is covered by an opt-in non-tokio probe, psql remains opt-in, tokio-postgres remains the deterministic default baseline, and sqlx/diesel/prisma/migration-tool automation remains planned depth.
- Issue 08 byte-accurate capacity diagnostics: `/metrics.capacity` reports advisory local key/value bytes by Midge family and by major Cassie read-model category without adding a second storage abstraction or automatic capacity action.

P2 follow-up:

9. Repair scope depth and operator runbooks.
10. Adaptive planning depth and promotion gates.

P3 parked:

11. Experimental surface promotion criteria.

## Required Gates

- Phase 04 runtime and access-path contracts are archived in `docs/performance-contracts.md` and `issues/phase-04/README.md`.
- Phase 05 write-path contracts are archived in `docs/performance-contracts.md` and `issues/phase-05/README.md`.
- Phase 06 read optimization contracts are archived in `issues/phase-06/README.md`.
- Phase 07 advanced query contracts are archived in `issues/phase-07/README.md`.
- Phase 08 README-goal closure is archived in `issues/phase-08/README.md`.
- Phase 09 issue 04 read-path depth is archived in `docs/performance-contracts.md`.
- Phase 09 issue 05 projection replay contracts are archived in `docs/projection-replay-contracts.md`.
- Phase 09 issue 06 bucket-native time-series storage is archived in `docs/indexes-and-constraints.md` and `docs/performance-contracts.md`.

## Non-Goals

- No distributed SQL execution.
- No cross-node query planning.
- No replication, quorum reads, consensus, or automatic remote repair.
- No second storage abstraction above Midge.
- No production-ready claim without the linked evidence required by `docs/production-readiness.md`.
