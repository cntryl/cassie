# Cassie Documentation

This directory documents Cassie as a query engine for event-sourced read models. The canonical documents have non-overlapping ownership:

- Root `README.md` owns Cassie's mission, boundaries, and navigation.
- `feature-support.md` owns behavior and status for every capability.
- `postgres-compatibility.md` owns pgwire and client compatibility.
- `performance-contracts.md` owns performance contracts, access paths, resource accounting, and benchmark evidence requirements.
- `production-readiness.md` owns readiness evidence and remaining Production-ready blockers.
- `product-roadmap.md` contains future work only.

Other documents may explain subsystem design or operator workflows. They must defer status, compatibility, performance, and readiness claims to the canonical owner above.

## Product and Query Contracts

- [Feature Support](feature-support.md)
- [PostgreSQL Compatibility](postgres-compatibility.md)
- [Performance Contracts](performance-contracts.md)
- [Production Readiness](production-readiness.md)
- [Product Roadmap](product-roadmap.md)
- [Indexes and Constraints](indexes-and-constraints.md)
- [Architecture Diagrams](architecture-diagrams.md)

Compile all benchmark owners with `cargo bench --locked --no-run --bench '*'`; run the normal developer suite with `cargo bench --locked --bench 'tier[1-4]_*'`. The Tier 1-6 contract and complete-suite commands live only in [Performance Contracts](performance-contracts.md).

## Projection and Operations

- [POC Quickstart](poc-quickstart.md)
- [Projection Replay Contracts](projection-replay-contracts.md)
- [Projection Repair Runbook](projection-repair-runbook.md)
- [Capacity Management](capacity-management.md)
- [Operational Scale](operational-scale.md)
- [Snapshot and Restore](snapshot-restore.md)
- [Database Families](database-families.md)

## Engineering

- [Definition of Done](definition-of-done.md)
- [Experimental Promotion Criteria](experimental-promotion-criteria.md)
- [Feature Ownership](feature-ownership.md)
- [Module Organization](module-organization.md)
- [Database Jargon Glossary](database-jargon-glossary.md)
- [Security Model](security-model.md)
