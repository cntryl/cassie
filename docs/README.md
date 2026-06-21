# Cassie Documentation

Cassie is a SQL-over-document-store database engine in Rust. It uses `cntryl-midge` directly as the storage layer, exposes PostgreSQL wire protocol as the primary query interface, and adds search, vector, hybrid, and analytical overlays on top of document-backed storage.

This directory is the product-facing source of truth for what Cassie supports, what compatibility it intends to provide, and how feature work reaches production-ready status.

## Start Here

- [Feature Support](feature-support.md): supported SQL, catalog, index, search, vector, analytics, protocol, and API surfaces.
- [Product Roadmap](product-roadmap.md): feature-area milestones and remaining roadmap themes.
- [PostgreSQL Compatibility](postgres-compatibility.md): compatibility guarantees, supported client surfaces, and intentional differences.
- [Definition of Done](definition-of-done.md): completion standards for implemented, experimental, and production-ready features.
- [Feature Ownership](feature-ownership.md): owning subsystems and default review boundaries for feature areas.
- [Indexes and Constraints](indexes-and-constraints.md): index, constraint, and analytical overlay behavior.
- [Module Organization](module-organization.md): code organization rules and large-file constraints.

## Product Posture

Most of the core query engine is implemented and tested. The main documentation job is no longer listing missing implementation work; it is making the implemented surface understandable, navigable, and explicit about compatibility guarantees.

Current supported areas include:

- Core SQL reads and writes.
- Table, schema, constraint, view, procedure, and catalog metadata.
- Scalar, composite, partial, expression, covering, full-text, vector, hybrid, and column-batch indexing surfaces.
- Full-text search, vector search, hybrid scoring, and embedding-provider integration.
- Column-batch scans, aggregate acceleration, time bucketing, rollups, EXPLAIN, metrics, and runtime diagnostics.
- PostgreSQL wire protocol basics, extended query flow, prepared statements, SQLSTATE-style errors, and catalog probes.

## Compatibility Language

Docs use these terms consistently:

- `Stable`: implemented, tested, documented, and intended to remain compatible within the same major line.
- `Experimental`: implemented or partially implemented, but behavior or compatibility may still change.
- `Planned`: roadmap item with no production compatibility guarantee yet.
- `Cassie-specific`: intentionally not PostgreSQL-compatible because the feature exposes Cassie storage, search, vector, AI, or analytics behavior.

## Updating Docs

When feature behavior changes, update the relevant docs in the same change:

- User-visible SQL or API behavior: update [Feature Support](feature-support.md).
- PostgreSQL/client compatibility behavior: update [PostgreSQL Compatibility](postgres-compatibility.md).
- Completion or support-level changes: update [Product Roadmap](product-roadmap.md) and [Definition of Done](definition-of-done.md).
- Index, constraint, or analytical overlay behavior: update [Indexes and Constraints](indexes-and-constraints.md).
- New subsystem ownership or file-layout decisions: update [Feature Ownership](feature-ownership.md) and [Module Organization](module-organization.md).
