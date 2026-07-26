# Local Diagnostics Contract

This document defines Cassie's local health, readiness, metrics, capacity, and operational
metadata fields. It is an operator and integration contract for one single-node Cassie process;
it is not a fleet, routing, placement, failover, or replication API.

## HTTP Health

The unauthenticated endpoints are intentionally small and contain no user data:

| Endpoint | Fields | Meaning |
| --- | --- | --- |
| `GET /healthz`, `/readyz`, `/startupz` | `status`, `ready`, `collections`, `version` | Kubernetes-style startup/readiness probes over local startup state. `status` is `ok` when `ready` is true and `starting` otherwise. |
| `GET /livez` | `ready` | Kubernetes-style liveness probe: the process is answering requests. It does not assert storage readiness. |

`/health` and `/liveness` remain compatibility aliases for existing clients.
Readiness-backed probes return HTTP `200` when `ready` is true and HTTP `503` while startup is incomplete.

Health fields are stable snake-case JSON names. `collections` is a count, not a collection-name
list, and health responses never include credentials, query text, row values, or tenant payloads.
The routes are suitable for local process and restart smoke checks; they do not establish a
production availability objective.

## Metrics

`GET /metrics` requires the normal authenticated admin boundary. `Cassie::metrics()` returns the
same object for embedded operators. The top-level fields include:

| Field | Meaning |
| --- | --- |
| `uptime_seconds` | Local process uptime. |
| `running_queries` | Currently admitted query workers. |
| `ready` | Current local startup state. |
| `runtime` and `query` | Admission, cancellation, latency, and query outcome counters. |
| `plan_cache`, `query_cache`, `execution_result_cache`, `feedback` | Cache occupancy, limits, and hit/miss or invalidation counters where applicable. |
| `storage` and `capacity` | Storage-family operations and advisory local logical byte usage. |
| `column_batches`, `projections`, `rollups`, `retention`, `read_paths` | Derived-state maintenance, rebuild, refresh, fallback, and selected read-path counters. |
| `rest` and `pgwire` | Local transport request/session and blocking-boundary counters. |

Metrics are aggregate diagnostics. Query text, credentials, row values, embedding payloads, and
raw tenant data are not emitted. Unknown fields may be added; existing field names retain their
meaning and JSON type.

## Capacity And Maintenance

`metrics.capacity` reports advisory logical key/value bytes for the local Midge data directory.
It includes stable `families` and `categories` objects, including `capacity.families` and
`capacity.categories`, with `supported`, `total_bytes`, and family/category-specific counters
where available. These values are not physical disk usage, compaction state, admission control,
or cross-node capacity.

Projection freshness, rebuild, verification, and repair pressure are queryable through the
documented `pg_catalog.pg_projection_operations`, `pg_catalog.pg_projection_integrity_reports`,
`pg_catalog.pg_projection_repair_reports`, and `pg_catalog.pg_maintenance_debt` views. Failed or
stale derived state must remain observable while query execution uses the authoritative fallback.

## Operational Assignments

`pg_catalog.pg_operational_assignments` is the queryable local assignment record. Its stable
fields are `assignment_id`, `node_id`, `projection_id`, `tenant`, `partition_key`, `generation`,
`state`, `routing_hint`, and `updated_ms`. `state` is one of `claimed`, `draining`, `released`,
or `failed`. Assignment metadata is persisted and hydrated on restart.

The view is metadata only. Cassie performs no routing, placement, movement, failover,
replication, fleet coordination, or remote repair. External orchestration owns those workflows;
see [Operational Scale](operational-scale.md).

## Evidence Boundary

`tests/rest_metrics.rs`, `tests/metrics_capacity.rs`, `tests/metrics_runtime.rs`,
`tests/operational_smoke.rs`, and `tests/operational_ownership.rs` provide local request,
capacity, runtime, restart, and assignment evidence. These tests establish field shape and local
behavior only. Production thresholds, sustained capacity, and availability claims require
retained deployment-profile evidence as described in [Capacity Management](capacity-management.md)
and [Production Readiness](production-readiness.md).
