# Projection Repair Runbook

Projection repair is an admin-only local workflow for fixing Cassie-owned projection hash metadata after `VERIFY PROJECTION` detects repairable row or range findings. It is never automatic, never part of query planning or execution, and does not perform distributed repair, replication, quorum reads, remote mutation, or cross-node reconciliation.

## Preconditions

- Run repair only on the Cassie instance that owns the local Midge data directory and projection metadata.
- Capture current `/metrics`, `EXPLAIN` output for the affected read path, and the latest integrity report from `pg_catalog.pg_projection_integrity_reports`.
- Run `VERIFY PROJECTION <name> MODE hashes_only` or `MODE full` before planning repair. Repair planning requires a persisted local integrity report.
- Keep traffic movement, snapshots, backups, and rollback orchestration outside Cassie.

## Plan

Use a dry-run plan first:

```sql
PLAN REPAIR PROJECTION projection_name SCOPE row;
PLAN REPAIR PROJECTION projection_name SCOPE range;
```

The plan returns the source report state, mismatch/missing/stale counts, intended action, whether the scope is executable, and the required follow-up verification command. Treat `executable = false` as a hard stop.

## Execute

Only `row` and `range` scopes are executable today. Both run the same local hash-rebuild action:

```sql
REPAIR PROJECTION projection_name SCOPE row;
REPAIR PROJECTION projection_name SCOPE range;
```

Cassie rebuilds local projection hashes for the target collection, then immediately runs `VERIFY PROJECTION <name> MODE full`. Successful repair persists an audit row in `pg_catalog.pg_projection_repair_reports` with `state = completed` and the post-verification state.

## Verify And Audit

After execution:

```sql
SELECT state, scope, action, post_verification_state
FROM pg_catalog.pg_projection_repair_reports
WHERE projection_name = 'projection_name';

VERIFY PROJECTION projection_name MODE full;
```

Proceed only when the latest repair report and integrity report are `verified`. If verification remains failed, stop serving the affected projection path or route externally according to the deployment policy.

## Unsupported Scopes

`index`, `projection-version`, and `full-rebuild` scopes are plan-only/error-only. They remain deterministic dry-run plans through `PLAN REPAIR PROJECTION`, but `REPAIR PROJECTION` rejects them until a future implementation specifies safe local mutation semantics, idempotency, audit fields, post-verification, and rollback behavior.

## Rollback Or Escalate

Cassie repair does not undo user data, copy data between nodes, or roll back projection versions. Escalate by restoring a local snapshot, replaying/rebuilding the projection, activating a verified projection version, or moving traffic to another independent node through the external router workflow. Record the integrity report id, repair report id, commands run, operator, timestamp, and follow-up verification state in the deployment incident log.
