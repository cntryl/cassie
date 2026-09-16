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

## Operator Actions By State

Each supported diagnostic state has one deterministic operator reaction. States not listed here
have no defined operator action and must not be inferred as safe to ignore.

| Surface | State | Meaning | Operator action |
| --- | --- | --- | --- |
| `/healthz`, `/readyz`, `/startupz` | `ready = false` | Startup has not finished. | Wait and re-poll; do not route traffic. Escalate if `ready` stays false past the expected startup window for the deployment profile. |
| `/livez` | `ready = false` | Process is not answering. | Restart the process; escalate if repeated restarts do not recover liveness. |
| `pg_catalog.pg_operational_assignments` | `claimed` | This node owns the assignment at the recorded generation. | Normal state; no action. Confirm `generation` is the highest known value before trusting the claim. |
| `pg_catalog.pg_operational_assignments` | `draining` | External orchestration is moving traffic off this assignment. | Stop routing new work externally; wait for in-flight work to finish, then mark `released`. Do not delete data while draining. |
| `pg_catalog.pg_operational_assignments` | `released` | This node no longer owns the assignment. | Safe to reclaim local resources for the assignment. Rollback: re-claim with a higher generation if validation on the new target failed. |
| `pg_catalog.pg_operational_assignments` | `failed` | The last claim/drain/release transition did not complete cleanly. | Manual intervention required: compare `generation`/`updated_ms` against the intended target, then re-issue a claim with a higher generation. Do not retry automatically. |
| `pg_catalog.pg_maintenance_debt` | Non-zero debt rising | Derived state (projection/rollup/index) is falling behind its source. | Run the documented rebuild/repair workflow during a maintenance window; escalate if debt keeps growing after rebuild. |
| `pg_catalog.pg_projection_integrity_reports` | Verification failure | A projection's derived rows disagree with the authoritative source. | Query continues to serve from the authoritative fallback. Run repair; treat repeated failures on the same projection as an escalation, not a routine retry. |
| `pg_catalog.pg_projection_repair_reports` | Repair failure | An attempted repair did not converge. | Do not retry automatically. Escalate for manual investigation; keep serving from the authoritative fallback in the meantime. |
| `capacity.families` / `capacity.categories` | Advisory threshold breached | See [Capacity Management](capacity-management.md) signal table. | Follow the matching response in that table; these are advisory, not release gates. |

These reactions describe local operator behavior only. Claiming, draining, releasing, or
retrying a remote node's assignment is external orchestration and is not part of this contract.

## Evidence Boundary

The `rest_metrics`, `metrics_capacity`, `metrics_runtime`, `operational_smoke`, and
`operational_ownership` modules in `tests/rest.rs`, `tests/metrics.rs`,
`tests/bench_operations.rs`, and `tests/storage_indexes.rs` provide local request,
capacity, runtime, restart, and assignment evidence. These tests establish field shape and local
behavior only. Production thresholds, sustained capacity, and availability claims require
retained deployment-profile evidence as described in [Capacity Management](capacity-management.md)
and [Production Readiness](production-readiness.md).

Retained operational manifests fail closed unless they identify the exact revision, deployment
profile, platform, and host resource context: CPU model, online core count, total memory,
filesystem type, and evidence-volume total and available bytes. That context supports comparison
between repeated same-profile runs; one runner snapshot is not itself a latency, throughput,
capacity, or availability commitment.
