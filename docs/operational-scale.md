# Operational Scale

Cassie scales operationally by running independent single-node read-model instances.
An external orchestrator decides where tenants, partitions, and projections should be served.
Cassie records local ownership and routing metadata so operators and orchestrators can inspect those assignments without adding distributed SQL behavior.

## External Router Contract

External routers read each Cassie node's local `pg_catalog.pg_operational_assignments` view and make routing decisions outside Cassie.
The view is an advisory contract between Cassie and fleet automation:

1. Poll or query every candidate node through pgwire.
2. Keep the highest `generation` per assignment scope.
3. Route new traffic only to assignments in `claimed`.
4. Stop new traffic to assignments in `draining`, but allow existing in-flight client work to finish according to router policy.
5. Treat `released` as not serving and remove it from the active routing table after the router observes a newer serving assignment or confirms no traffic remains.
6. Treat `failed` as not serving and require operator or orchestrator action before routing resumes.

Cassie does not subscribe routers, push assignment changes, forward queries, or validate that a client chose the right node.
Routers should use their own freshness deadlines for assignment polling and should fail closed when assignment metadata is missing, stale, contradictory, or lower generation than another observed record for the same scope.

## Local Assignment Metadata

`pg_catalog.pg_operational_assignments` exposes local assignment records:

| Column | Meaning |
| --- | --- |
| `assignment_id` | Stable assignment identifier. Use one id per router-owned assignment scope. |
| `node_id` | Node identity that claims, drains, releases, or reports failure for the local assignment. |
| `projection_id` | Projection or collection served by the assignment. Routers use this as the read-model target. |
| `tenant` | Optional tenant routing key. Empty means the assignment is not tenant-scoped. |
| `partition_key` | Optional partition assignment key within the tenant or projection scope. Empty means the assignment is not partition-scoped. |
| `generation` | Monotonic generation supplied by the orchestrator for the assignment scope. Higher generations supersede lower generations. |
| `state` | Local assignment state: `claimed`, `draining`, `released`, or `failed`. |
| `routing_hint` | Optional opaque value for router configuration, such as endpoint group, shard label, or route policy id. Cassie stores it but never interprets it. |
| `updated_ms` | Assignment update timestamp in milliseconds, supplied by the caller that writes the metadata. Routers can use it for freshness checks. |

Assignment metadata is persisted in Midge schema storage and hydrated during startup.
It is diagnostic and administrative metadata only.

## Assignment Lifecycle

`claimed` means the local node is expected to serve the assignment.
Routers can send new traffic to the node when the observed assignment is the highest current generation for the scope.

`draining` means the local node should not receive new traffic for the assignment.
Routers should remove it from new-request routing, wait for their own in-flight requests to finish, and continue observing the node until the assignment becomes `released` or `failed`.

`released` means the local node no longer serves the assignment.
Routers should not send traffic to it and can discard it after a newer `claimed` assignment is active elsewhere.

`failed` means the local node reports that the assignment cannot be served safely.
Routers should not send traffic to it.
Recovery is an external operator workflow: restore, rebuild, replay, repair, or route to another independent node according to deployment policy.

Generations are scoped by the orchestrator's assignment model.
For tenant movement, use a generation per tenant/projection or tenant/projection/partition scope.
For projection ownership, use a generation per projection or projection version scope.
Cassie compares no generations in the query path; external routers must choose the winning assignment.

## Movement Workflow

Use this workflow when moving a tenant, partition, or projection between independent Cassie nodes:

1. Prepare the target node with the required projection data, indexes, catalog objects, and any replay or restore work needed by the deployment.
2. Write a target assignment with a higher `generation`, state `claimed`, and the target node's `node_id`.
3. Wait for the target node to report the assignment through `pg_catalog.pg_operational_assignments`.
4. Update the external router so new traffic for the assignment scope goes to the target node.
5. Mark the source assignment `draining` at a generation that is not lower than the target generation.
6. Wait for router-side in-flight traffic to drain.
7. Mark the source assignment `released`.
8. Keep enough monitoring and snapshot/replay evidence to roll back by claiming a still-valid source or alternate node with a newer generation.

Rollback is the same contract in reverse: claim a valid node at a higher generation, move router traffic externally, drain the previous serving node, then release it.
Cassie does not coordinate the move and does not copy data between nodes.

## Capacity Movement

Capacity movement uses the same assignment lifecycle.
Operators should combine this view with [Capacity Management](capacity-management.md) signals:

- Move a hot tenant or projection when router-side per-tenant latency, Cassie query latency, pgwire/REST blocking elapsed time, or candidate counts rise beyond the deployment profile's tolerance.
- Isolate rebuild or verification work by claiming serving traffic on another prepared node, marking the busy assignment `draining`, and running local admin work after traffic leaves.
- Move before disk pressure becomes critical; use host free-space measurements for `CASSIE_MIDGE_DATA_DIR`, storage-family operation counters, projection write counters, and snapshot headroom.
- Prefer targeted tenant/projection movement over broad fleet reshaping when only one read model or tenant is hot.

## Query Behavior

Assignment metadata never routes, filters, fans out, or rewrites SQL queries.
Queries continue to execute against the local Cassie node and local Midge data.
If an external router sends traffic to the wrong node, Cassie does not issue remote reads or forward the query.
Cassie also does not replicate assignment data to other nodes, elect owners, or wait for quorum before executing local SQL.

## Non-Goals

- No distributed SQL execution.
- No cross-node query planning.
- No replication or quorum reads.
- No consensus or leader election.
- No automatic repair or remote mutation.

External orchestration owns node placement, tenant routing, partition assignment, failover policy, and traffic movement.
Use [Capacity Management](capacity-management.md) for the advisory signals that help decide when to move tenants, isolate rebuild work, or add independent read nodes.
