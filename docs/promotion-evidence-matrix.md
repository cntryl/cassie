# Promotion Evidence Matrix

This matrix tracks the shared promotion gate for issues #21-#29. A row is **not promoted**
until implementation, compatibility, restart, diagnostics, and retained same-commit evidence
are all present. Local tests prove correctness slices; they do not prove hosted client support,
representative capacity, latency, or production readiness.

| Issue | Surface and current evidence | Missing closure evidence | Status |
| --- | --- | --- | --- |
| #21 | Catalog and virtual-view schemas, hydration, rename/drop cleanup, and client probes exist in `tests/catalog_introspection*.rs`, `tests/catalog_probes.rs`, and `tests/catalog_orm_metadata.rs`. | Named client compatibility runs and a declared stable subset of views with retained hosted output. | Not promoted; keep partial PostgreSQL parity explicit. |
| #22 | Limited procedures and `CALL` cover argument binding, restart hydration, metadata, recursion, and transaction-control rejection in `tests/executor_commands.rs` and `tests/compatibility_matrix.rs`. | A published narrow body/argument contract and pgwire client evidence for the supported subset. | Not promoted; business logic, PL/pgSQL, triggers, dynamic SQL, and transaction control remain unsupported. |
| #23 | Rollup/retention lifecycle, stale fallback, maintenance debt, restart, and metrics coverage exist in `tests/time_series_rollups.rs`, `tests/time_series_retention.rs`, and related recovery suites. | Retained idempotency and operator-window benchmark evidence at declared fixture sizes, including dependent freshness. | Not promoted. |
| #24 | HNSW option validation, persistence, mutation safety, exact rerank, fallback, dimensions, and recall checks exist in `tests/hnsw_indexes.rs` and `tests/vector_query_stability.rs`. | Same-commit retained recall/latency benchmarks and provider-specific deployment evidence. | Not promoted. |
| #25 | IVFFlat training persistence, refresh, list validation, fallback, restart, and recall checks exist in `tests/ivfflat_indexes.rs`, `tests/ivfflat_completeness.rs`, and `tests/vector_query_stability.rs`. | Same-commit retained recall/latency benchmarks across declared list/probe profiles. | Not promoted. |
| #26 | Provider controls, response bounds, retries, cancellation, dimensions, models, and self-hosted protocols are covered in `tests/embedding_*`, `tests/rest_embeddings.rs`, and the feature-support contract. | Resolve the Stable/Experimental contradiction per provider, then retain hosted or explicitly mocked auth/rate-limit evidence. | Not promoted; external availability is not claimed. |
| #27 | Time-series range, mutation, retention, restart, and fallback paths are covered in `tests/time_series_indexes.rs`, `tests/time_series_rollups.rs`, and `tests/time_series_retention.rs`. | Retained benchmark evidence across supported bucket widths and dependent retention windows. | Not promoted. |
| #28 | Analytical projections and CBM2 column paths have DML, freshness, restart, cleanup, EXPLAIN, corruption, and authoritative-row fallback tests. | Retained analytical fixture benchmarks with capacity, codec, and DML amplification evidence on the release commit. | Not promoted. |
| #29 | Local diagnostics, capacity categories, maintenance debt, repair reports, and operational assignments are documented and covered by `tests/metrics_*`, `tests/operational_*`, and projection repair suites. | Retained deployment-profile thresholds and recovery/repair/capacity evidence; routing, movement, admission, replication, and distributed repair remain external. | Not promoted; local-only boundary is intentional. |

The common gates are defined in [Experimental Promotion Criteria](experimental-promotion-criteria.md).
Promotion records must link exact commits, toolchains, fixture/profile ids, commands, artifacts,
and unresolved blockers in [Production Readiness](production-readiness.md).
