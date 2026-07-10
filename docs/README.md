# Cassie Documentation

Cassie is a high-performance read-model database for event-sourced systems. It uses `cntryl-midge` directly as the storage layer, exposes PostgreSQL wire protocol as the primary query interface for tools and applications, and adds search, vector, hybrid, and analytical overlays on top of document-backed projection storage.

This directory is the product-facing source of truth for what Cassie supports, what compatibility it intends to provide, and how feature work reaches production-ready status.

## Start Here

- [POC Quickstart](poc-quickstart.md): shortest local proof that Cassie can serve an embedded read-model workload.
- [Feature Support](feature-support.md): supported SQL, catalog, index, search, vector, analytics, protocol, and API surfaces.
- [Product Roadmap](product-roadmap.md): feature-area milestones and remaining roadmap themes.
- [Read-Model Gap Analysis](read-model-gap-analysis.md): strategic gaps against Cassie's event-sourced read-model mission.
- [Performance Contracts](performance-contracts.md): read-model query-pattern contracts, execution expectations, and benchmark ownership.
- [Projection Replay Contracts](projection-replay-contracts.md): deterministic handler, replay failure, and operator recovery boundaries.
- [Projection Repair Runbook](projection-repair-runbook.md): admin-only local repair planning, execution, verification, audit, and escalation workflow.
- [Capacity Management](capacity-management.md): advisory sizing signals, thresholds, and operator actions for single-node read-model deployments.
- [Read-Model Autopilot Plan](read-model-autopilot-plan.md): archived execution rules from the gap-closure rebaseline.
- [PostgreSQL Compatibility](postgres-compatibility.md): compatibility guarantees, supported client surfaces, and intentional differences.
- [Production Readiness](production-readiness.md): feature-family readiness, evidence, operational signals, restart coverage, and blockers.
- [Experimental Promotion Criteria](experimental-promotion-criteria.md): evidence gates for promoting or narrowing experimental surfaces.
- [Operational Scale](operational-scale.md): local assignment metadata and external router/drain/move contracts for independent read nodes.
- [Snapshot And Restore](snapshot-restore.md): local Midge-directory snapshots with Cassie compatibility manifests.
- [Definition of Done](definition-of-done.md): completion standards for implemented, experimental, and production-ready features.
- [Feature Ownership](feature-ownership.md): owning subsystems and default review boundaries for feature areas.
- [Indexes and Constraints](indexes-and-constraints.md): index, constraint, and analytical overlay behavior.
- [Architecture Diagrams](architecture-diagrams.md): Mermaid module, execution, operational surface, and drift-analysis reference.
- [Module Organization](module-organization.md): code organization rules and large-file constraints.

## Product Posture

Cassie is not intended to be a general-purpose OLTP database competing with PostgreSQL, MySQL, or SQL Server. In the CNTRYL architecture, the event stream is the system of record; Cassie exists to materialize, query, search, analyze, and serve projections derived from that stream.

Feature prioritization is driven by read-model requirements rather than database taxonomy. A capability is in scope when users need it to build, operate, analyze, search, report on, or serve event-sourced read models, regardless of whether the capability originates from relational, analytical, search, vector, or time-series workloads.

Most of the core query engine is implemented and tested. The main documentation job is now to make the implemented surface understandable, navigable, and explicit about read-model guarantees, projection lifecycle behavior, and compatibility boundaries.

Current supported areas include:

- Core SQL reads and projection-state mutation workflows.
- Projection checkpoints, replay diagnostics, materialized projections, versioned builds, and active-version swaps.
- Verification, repair planning, local hash repair, and repair audit reporting.
- Database, schema, `search_path`, constraint, view, limited procedure, and catalog metadata.
- Scalar, composite, partial, expression, covering, full-text, vector, hybrid, and column-batch indexing surfaces.
- Full-text search, vector search, hybrid scoring, and embedding-provider integration.
- Column-batch scans, aggregate acceleration, time bucketing, rollups, EXPLAIN, metrics, and runtime diagnostics.
- PostgreSQL wire protocol basics, extended query flow, prepared statements, SQLSTATE-style errors, catalog probes, tokio-postgres baseline coverage, and opt-in psql/Prisma/SQLAlchemy probes.

Current PostgreSQL-like session/database model:

- Fresh startup bootstraps the configured default database and persisted `public` schema.
- Each session is bound to one existing database; `current_database()`, `current_schema()`, `SHOW search_path`, and `SET search_path` follow that session scope.
- Unqualified names resolve through the session `search_path` within the current database only.
- Cross-database `database.schema.relation` references are intentionally unsupported.
- Cassie-owned Midge layout is a clean-break lexkey `v4`; older flat or `v1`/`v2`/`v3` data directories must be recreated instead of migrated in place.
- Local operational assignment metadata plus external route, drain, move, failure, and rollback contracts for node, tenant, partition, and projection routing.
- Local snapshot and restore workflow for single-node Midge-backed recovery.
- Advisory capacity-management guidance using metrics, EXPLAIN, catalog diagnostics, host disk measurements, and manual benchmark scenarios.

## Compatibility Language

Docs use these terms consistently:

- `Stable`: implemented, tested, documented, and intended to remain compatible within the same major line.
- `Experimental`: implemented or partially implemented, but behavior or compatibility may still change.
- `Planned`: roadmap item with no production compatibility guarantee yet.
- `Cassie-specific`: intentionally not PostgreSQL-compatible because the feature exposes Cassie storage, search, vector, AI, or analytics behavior.

Experimental surfaces promote only through the evidence gates in [Experimental Promotion Criteria](experimental-promotion-criteria.md).

## Updating Docs

When feature behavior changes, update the relevant docs in the same change:

- User-visible SQL or API behavior: update [Feature Support](feature-support.md).
- PostgreSQL/client compatibility behavior: update [PostgreSQL Compatibility](postgres-compatibility.md).
- Completion or support-level changes: update [Product Roadmap](product-roadmap.md) and [Definition of Done](definition-of-done.md).
- Capacity, sizing, threshold, or operational-signal changes: update [Capacity Management](capacity-management.md) and [Production Readiness](production-readiness.md).
- Index, constraint, or analytical overlay behavior: update [Indexes and Constraints](indexes-and-constraints.md).
- New subsystem ownership or file-layout decisions: update [Feature Ownership](feature-ownership.md) and [Module Organization](module-organization.md).
