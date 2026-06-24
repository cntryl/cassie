# Capacity Management

Cassie capacity management is an operator feedback loop for single-node read-model deployments.
It helps decide when to reshape projections, add or remove indexes, move tenants to another independent node, schedule rebuild work, or collect stronger benchmark evidence.

This is a documented baseline, not a production SLA or automatic admission-control system.
Cassie exposes advisory logical key/value byte usage through `/metrics.capacity` for the local Midge data directory.
Use that report together with host disk measurements for `CASSIE_MIDGE_DATA_DIR`, EXPLAIN diagnostics, catalog views, and benchmark scenarios.
Deployment-profile evidence is advisory until a production owner records targets and thresholds for that profile.

## Signal Sources

| Source | Use |
| --- | --- |
| `GET /metrics` or `Cassie::metrics()` | Runtime, query, storage-family operation counts, cache occupancy, fallback counters, projection work, retention, rollup, time-series, search, vector, hybrid, pgwire, REST signals, and local advisory capacity bytes. |
| `EXPLAIN` and `EXPLAIN ANALYZE` | Access path, fallback reason, storage mode, selected index, rollup rewrite, time-series bucket diagnostics, projection freshness, and candidate strategy. |
| `pg_catalog.pg_operational_assignments` | Local tenant, partition, node, and projection assignment metadata for external routers. |
| `pg_catalog.pg_projection_operations` | Projection version, freshness, checkpoint, lag, rebuild, verification, and last-error state. |
| `pg_catalog.pg_table_storage` | Effective table storage mode: row-store, column-indexed, or column-store. |
| `pg_catalog.pg_projection_integrity_reports` and `pg_catalog.pg_projection_repair_reports` | Verification and repair pressure after rebuilds or divergence checks. |
| [Performance Contracts](performance-contracts.md) | Manual 10k/100k benchmark scenarios, deployment-profile ids, and the evidence labels that tie workloads to metrics. |

## Capacity Dimensions

| Dimension | Watch | Operator action |
| --- | --- | --- |
| CPU | Rising `query.latency_ms_total / query.count`, high `pgwire.blocking_elapsed_ms_total` or `rest.blocking_elapsed_ms_total`, growing search/vector/hybrid candidate counts, join/aggregate row counters, and repeated parallel fallback counters. | Add or reshape indexes, materialize projection-specific read shapes, use rollups/column batches for analytical paths, reduce candidate fan-out, or move hot tenants/projections to another independent node. |
| Memory | `runtime.running_queries`, `plan_cache.entries / plan_cache.max_entries`, `feedback.entries / feedback.max_entries`, query-cache miss rates, candidate counts, vectorized join batch size, and row-blob fallback counts. | Bound result sets, lower query concurrency externally, reduce candidate budgets, split hot read models, tune cache limits, or replace broad interactive scans with projection-shaped reads. |
| Disk and storage IO | Host size and free space for `CASSIE_MIDGE_DATA_DIR`, `/metrics.capacity` family/category byte totals, `storage.schema.reads`, `storage.data.reads`, `storage.temp.reads`, corresponding storage writes, storage errors, projection row/index write counters, retention deletes/skips, and column-batch compressed/uncompressed byte totals. | Keep free-space headroom for snapshots and rebuild targets, remove unused indexes, enforce retention deliberately, schedule compaction or host-level cleanup according to Midge guidance, and move tenants before free space or write IO becomes the limiting resource. |
| Index overhead | `projections.write_index_puts`, `projections.write_index_deletes`, `read_paths.index_seek_scans`, `read_paths.prefix_scans`, `read_paths.range_scans`, `covering_indexes.row_fetches_avoided`, covering-index fallbacks, vector fallback reasons, and column-batch bytes. | Keep indexes that serve documented read-model paths, remove indexes that add write cost without read usage, prefer composite/covering indexes for hot pages, and treat vector/column-batch structures as capacity-bearing sidecars. |
| Projection count and rebuild pressure | Projection catalog rows, `projections.materialized_builds`, `projections.materialized_refreshes`, `projections.write_rebuild_target_puts`, `projections.version_swaps`, `projections.stale_marks`, verification counters, and mixed-execution fallbacks. | Cap concurrent rebuild work externally, run rebuilds outside hot serving windows, keep inactive rebuild targets within disk headroom, verify before swap, and split heavy projection families across independent Cassie nodes. |
| Tenant and partition load | `pg_catalog.pg_operational_assignments`, router-side per-tenant QPS/latency, hot collection names in read-path diagnostics, pgwire active sessions, and REST/pgwire route or protocol counters. | Route tenants to the node that owns their local assignment, mark assignments draining before traffic movement, add independent nodes for isolated hot tenants, and keep Cassie out of cross-node query routing. |
| Cache occupancy | `plan_cache.entries`, `plan_cache.max_entries`, plan-cache hits/misses/evictions, `feedback.entries`, `feedback.max_entries`, query-cache hits/misses, schema-epoch rejects, and deserialize rejects. | Increase documented cache limits only when memory headroom exists, reduce query-shape churn, prefer prepared/parameterized access patterns through pgwire clients, and watch invalidations after schema or index churn. |
| Fallback rate | `fallback_reason` in EXPLAIN, `read_paths.degraded_offset_scans`, column-batch fallback counters, rollup stale fallbacks, time-series fallback scans, vector/hybrid prefilter fallback counts, join fallbacks, and projection mixed-execution fallbacks. | Treat repeated fallback as capacity debt: add the missing index or projection shape, refresh stale rollups/projections, fix unsupported predicates, or document the degraded path as intentionally batch/offline. |

## Advisory Thresholds

These thresholds are starting points for manual operations and local development feedback.
Tune them per deployment profile before using them as alerts.
The current `local-dev-fallback-10k` and `local-dev-fallback-100k` profiles provide repeatable developer feedback, not admission control or SLA evidence.

| Signal | Advisory threshold | Response |
| --- | --- | --- |
| Disk free space under `CASSIE_MIDGE_DATA_DIR` | Below 30% during normal serving, or below 50% before snapshots, restore tests, large index builds, or projection rebuilds. | Add capacity, move tenants, reduce retention horizon, or defer rebuild/snapshot work. |
| Plan or feedback cache occupancy | Above 90% for sustained traffic, or frequent evictions with rising query latency. | Raise cache limits if memory allows, normalize query shapes, or reduce schema/index churn. |
| Row-blob or degraded fallback rate | Any repeated fallback on an interactive read-model path that is expected to use an index, rollup, time-series path, column batch, or projection. | Use EXPLAIN to identify the reason, add the required access path, or move the workload to an offline/reporting profile. |
| Rebuild target writes | Rebuild write counters rising while interactive latency also rises. | Isolate rebuild windows, reduce concurrent rebuilds, or move serving traffic to another node before rebuild. |
| Retention work | Large `retention.deleted_rows` or `retention.skipped_rows` spikes near hot serving windows. | Run retention explicitly during maintenance windows and verify dependent rollups/projections after enforcement. |
| Search/vector/hybrid candidates | Candidate totals grow faster than result totals or p95/p99 benchmark results regress at the same fixture scale. | Tighten structured prefilters, tune vector index options, materialize a narrower projection, or split retrieval workloads. |
| Pgwire/REST blocking elapsed time | Blocking elapsed totals rise faster than request/query counts. | Check slow SQL, auth/provider paths, client concurrency, and whether expensive admin work is sharing the serving node. |

## External Capacity Movement

Capacity movement is external orchestration over independent Cassie nodes.
Cassie exposes the local assignment view and local metrics, but it does not move tenants, rebalance partitions, copy projection data, or route queries.

Use [Operational Scale](operational-scale.md) as the assignment lifecycle contract:

1. Identify the overloaded scope from router-side tenant/projection metrics, Cassie latency and fallback metrics, EXPLAIN diagnostics, host resource usage, and `pg_catalog.pg_operational_assignments`.
2. Prepare a target independent node with the required projection data and access paths.
3. Claim the target assignment with a higher generation.
4. Move new router traffic externally.
5. Mark the source assignment `draining`, wait for router-side in-flight work to finish, then mark it `released`.
6. Keep rollback simple by claiming a known-good node with a newer generation if target validation fails.

Use targeted movement before broad fleet changes.
Move a tenant when one tenant dominates query latency or candidate counts.
Move a projection when rebuild, verification, rollup, search, vector, or analytical work for that projection pressures the serving node.
Move before disk pressure blocks snapshots, restores, index builds, projection rebuilds, or Midge maintenance.

## Sizing Workflow

1. Start from the query shapes in [Performance Contracts](performance-contracts.md), not from generic database benchmarks.
2. Choose the closest deployment profile, usually `local-dev-fallback-10k` for fast feedback or `local-dev-fallback-100k` for heavier local evidence.
3. Run the matching manual Criterion scenarios for the feature family you are changing and keep the profile id in the report line.
4. Capture `/metrics`, including `capacity.families` and `capacity.categories`, representative `EXPLAIN ANALYZE` output, host CPU, memory, and `CASSIE_MIDGE_DATA_DIR` disk usage before and after the change.
5. Compare fallback counters, cache occupancy, candidate counts, storage-family operations, advisory capacity bytes, and rebuild/write-amplification counters against earlier evidence for the same profile.
6. Decide whether to add an access path, reshape the projection, move a tenant/projection to another independent node, or keep the workload as explicit batch/offline work.

## Current Limits

- Cassie does not perform automatic tenant movement, admission control, distributed routing, replication, quorum reads, or cross-node repair.
- `/metrics.capacity` reports advisory logical key/value bytes by Midge family and by major Cassie category: row blobs, scalar indexes, full-text metadata, vector sidecars, column batches, projection metadata, temporary artifacts, and other data.
- Capacity bytes are local to one Cassie data directory and are not a physical disk-usage, compaction, replication, movement, or admission-control contract.
- Capacity guidance is advisory until a deployment profile records benchmark targets, host profile, data shape, workload mix, and operator thresholds.
