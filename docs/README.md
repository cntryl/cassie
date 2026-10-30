# Cassie Documentation

This directory documents Cassie as a single-node read-model database for CQRS and event-sourced systems. The [Product Roadmap](product-roadmap.md) is the canonical owner of the product goal, scope, exclusions, and release order. PostgreSQL 18.6 is used as a semantic reference for selected behavior; compatibility documents and test corpus do not promise full PostgreSQL parity.

The canonical documents have non-overlapping ownership:

- Root `README.md` owns Cassie's mission, boundaries, and navigation.
- `feature-support.md` owns behavior and status for every capability.
- `postgres-compatibility.md` owns pgwire and client compatibility.
- `performance-contracts.md` owns performance contracts, access paths, resource accounting, and benchmark evidence requirements.
- `production-readiness.md` owns readiness evidence and remaining Production-ready blockers.
- `product-roadmap.md` owns Cassie's product scope, release order, exclusions, and future work.
- `release-baseline.md` records the unreleased 0.1.0 starting point and Cassie-owned format boundary.

Other documents may explain subsystem design or operator workflows. They must defer status, compatibility, performance, and readiness claims to the canonical owner above.

## Product and Query Contracts

- [Feature Support](feature-support.md)
- [PostgreSQL Compatibility](postgres-compatibility.md)
- [Finite SQL Type and Wire Contract](type-contract.md)
- [Scoped SQL Dialect Profile](sql-dialect-profile.md)
- [Internal Typed Batch Contract](typed-batch-contract.md)
- [First Typed Pipeline Acceptance](typed-pipeline-acceptance.md)
- [PostgreSQL Wire Transaction Boundaries](pgwire-transaction-contract.md)
- [Query Promotion Evidence](query-promotion-evidence.md)
- [Compatibility Probe Contract](compatibility-probe-contract.md)
- [Performance Contracts](performance-contracts.md)
- [Query Result Ownership](query-result-ownership.md)
- [HNSW Release Evidence](hnsw-release-evidence.md)
- [IVFFlat Release Evidence](ivfflat-release-evidence.md)
- [Production Readiness](production-readiness.md)
- [Support, Release, and Security-Response Policy](support-policy.md)
- [Product Roadmap](product-roadmap.md)
- [0.1.0 Release Baseline](release-baseline.md)
- [Indexes and Constraints](indexes-and-constraints.md)
- [Recoverable Vector Index Publication](vector-index-publication.md)
- [Architecture Diagrams](architecture-diagrams.md)

Compile all benchmark owners with `cargo bench --locked --no-run --bench '*'`; run the normal developer suite with `cargo bench --locked --bench 'tier[1-4]_*'`. The Tier 1-6 contract and complete-suite commands live only in [Performance Contracts](performance-contracts.md).

## Projection and Operations

- [POC Quickstart](poc-quickstart.md)
- [Environment Variables](environment-variables.md)
- [Projection Replay Contracts](projection-replay-contracts.md)
- [Materialized Projection Lifecycle](materialized-projection-lifecycle.md)
- [Projection Repair Runbook](projection-repair-runbook.md)
- [Capacity Management](capacity-management.md)
- [Local Diagnostics Contract](local-diagnostics-contract.md)
- [Operational Scale](operational-scale.md)
- [Snapshot and Restore](snapshot-restore.md)
- [Database Families](database-families.md)

## Engineering

- [Definition of Done](definition-of-done.md)
- [Release Checklist](release-checklist.md)
- [Tooling Contract](tooling-contract.md)
- [Completed Ordinary Aliases and Pagination Integration Record](alias-pagination-plan.md)
- [Experimental Promotion Criteria](experimental-promotion-criteria.md)
- [Promotion Evidence Matrix](promotion-evidence-matrix.md)
- [Feature Ownership](feature-ownership.md)
- [Module Organization](module-organization.md)
- [Database Jargon Glossary](database-jargon-glossary.md)
- [Security and Response Policy](support-policy.md)

## Compatibility reference and historical records

- [Scoped SQL dialect profile](sql-dialect-profile.md) — active only for behaviors selected by the roadmap and current issue criteria.
- [PostgreSQL 18 differential corpus](postgresql18-corpus.md) — comparison harness and retained evidence for selected behavior; not a parity target.
- [Former PostgreSQL 18 delivery plan](postgresql18-delivery.md) — superseded historical record.
- [Former expanded SQL dialect profile](sql-dialect-profile-history.md) — historical proposal and implementation notes.
- [Former parity gap analysis](postgres-sql-parity-gap-analysis.md) — superseded historical survey.
- [Former parity backlog snapshot](dialect-backlog-status.md) — stale point-in-time inventory; GitHub is the live backlog.
- [Former PostgreSQL 18 test inventory](postgresql18-test-inventory.md) — historical test and owner snapshot.
- [Pre-extension #752 type-contract checkpoint](type-contract-history.md), [former type and durable-format decisions](extended-type-decisions.md), [type-family records](postgresql18-type-families.md), [type-resolution contract](postgresql18-type-resolution-resources.md), [v3 proposal](postgresql18-v3-contract.md), and [catalog/export proposal](postgresql18-catalog-export-proposal.md) — preserved design history, not current product requirements or implemented storage contracts.
- [Former query-engine delivery tracks](query-engine-delivery-tracks.md) — superseded work assignment and sequencing snapshot.

Historical records preserve provenance. Their old issue states and acceptance plans do not override the roadmap, current feature-support document, current compatibility contract, or live GitHub milestones.
