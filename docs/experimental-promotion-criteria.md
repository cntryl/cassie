# Experimental Promotion Criteria

Experimental Cassie surfaces are usable for supported cases, but their compatibility, output shape, operational envelope, or production evidence may still change. This document defines the evidence required before a future issue may promote one surface to `Stable` or `Production-ready`.

Promotion must happen one surface at a time. Do not promote a feature family only because the code exists or has broad integration coverage.

## Common Gates

Every promotion issue must provide:

- Supported and unsupported behavior in [Feature Support](feature-support.md).
- Deterministic errors for reachable unsupported paths.
- Restart, hydration, rename/drop, and cleanup tests when metadata persists.
- EXPLAIN, metrics, catalog, or protocol diagnostics for non-obvious planner/executor behavior.
- Compatibility notes for pgwire-visible SQL, catalog, or error behavior.
- Benchmark or deployment-profile evidence for performance-sensitive paths.
- Operator documentation for admin, recovery, repair, capacity, or external orchestration workflows.
- A production-readiness row update that links the exact evidence and leaves unresolved blockers in place.

## Surface Criteria

| Surface | Current posture | Promotion requirement |
| --- | --- | --- |
| Catalog metadata and virtual views | Experimental where PostgreSQL parity is partial. | Stable only for views with documented columns, deterministic ordering, restart coverage, SQLSTATE behavior for unsupported probes, and client compatibility evidence. Keep PostgreSQL-internal parity claims out of scope unless explicitly implemented. |
| Limited procedures and `CALL` | Experimental compatibility/admin surface. | Keep narrow. Promote only if supported body semantics, argument binding, restart hydration, pgwire metadata, and deterministic rejection of PL/pgSQL, triggers, dynamic SQL, transaction-control procedures, recursion, and business-logic workflows remain documented and tested. |
| Rollups and retention | Experimental explicit operational workflow. | Promote after refresh/enforcement idempotency, restart hydration, stale fallback, dependent projection freshness, metrics, and benchmark evidence exist for declared fixture sizes and operator windows. |
| HNSW vector indexes | Experimental graph candidate path with exact re-rank. | Promote only after option validation, restart hydration, exact rerank correctness, recall/latency benchmarks, fallback diagnostics, and provider/dimension mismatch handling are documented. |
| IVFFlat vector indexes | Experimental exact rerank path over trained candidates. | Promote after training persistence, refresh behavior after writes, list/probe option validation, recall/latency evidence, restart coverage, and deterministic untrained/fallback behavior. |
| Embedding providers | Experimental external dependency surface. | Keep provider-specific compatibility explicit. Promote a provider only with auth/config docs, timeout/retry behavior, dimension/model validation, deterministic ingest/query failures, self-hosted and hosted tests or mocks, and operational guidance for rate limits. |
| Time-series indexes | Experimental Cassie-specific bucket membership. | Promote after range planning, mutation/delete/retention correctness, restart coverage, bucket fallback diagnostics, and benchmark evidence across supported bucket widths. |
| Analytical projections and column-store table mode | Experimental Cassie-specific storage/read path. | Promote only with DML boundary tests, freshness/fallback semantics, restart and cleanup coverage, EXPLAIN labels, capacity evidence, and analytical fixture benchmarks. |
| Adaptive planning and operator switching | Experimental internal feedback path. | Keep disabled by default until production profiles define confidence/cost thresholds, stale-feedback behavior, result-equivalence tests, EXPLAIN guard diagnostics, and rollback controls. |
| Operational metadata and capacity diagnostics | Experimental operational surface. | Promote only as local metadata/diagnostics. External routing, movement, capacity-based admission control, and distributed repair must remain external unless a separate roadmap issue changes that boundary. |

## Retain Or Narrow

Some surfaces should remain experimental even with strong tests if their compatibility is intentionally Cassie-specific or their output shape is expected to evolve. For these, prefer narrowing scope over promotion:

- Procedures must remain limited compatibility/admin workflows, not a stored-procedure business-logic platform.
- Catalog probes may be stable for named client workflows without claiming full PostgreSQL catalog parity.
- EXPLAIN and metrics fields may be stable by documented key while preserving room for additional diagnostics.
- External-provider behavior must remain tied to documented provider contracts and failure modes, not general availability guarantees.

Future promotion issues should cite this document, update only the selected surface, and leave unrelated experimental surfaces unchanged.
