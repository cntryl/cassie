# Materialized Projection Lifecycle

This document defines Cassie's supported local lifecycle for analytical materialized projections.
It does not change the separate Stable CBM2 column-batch contract. Projection creation, rebuild,
verification, activation, fallback, and repair operate on one Cassie instance and its Midge data
directory; replication, remote coordination, and traffic movement remain external.

## Supported Lifecycle

1. `CREATE MATERIALIZED PROJECTION ... AS SELECT ...` persists the definition, builds `v1`, and
   activates it only after output rows and their row, range, and root hashes are durable.
2. Source writes mark dependent projection metadata stale. Reads that cannot prove current source
   generations use authoritative source rows rather than an old projection output.
3. `REFRESH MATERIALIZED PROJECTION <name>` rebuilds the active version under source and output
   write gates, publishes hashes, and then publishes fresh metadata.
4. `ALTER MATERIALIZED PROJECTION <name> BUILD VERSION` creates the next inactive version. Version
   ids come from a persisted high-water ordinal and are never reused, even after `DROP ... VERSION`.
   A failed or interrupted build is recorded as failed and never becomes query-visible.
5. `VERIFY PROJECTION <name> [VERSION <id>] MODE <mode>` persists an integrity report. Supported
   modes are `metadata_only`, `hashes_only`, `indexes_only`, and `full`.
6. `ALTER MATERIALIZED PROJECTION <name> ACTIVATE VERSION <id>` atomically retires the previous
   active version and activates a built target. Publication is one synchronous metadata commit.
7. `DROP MATERIALIZED PROJECTION VERSION <name> VERSION <id>` removes an inactive version, its
   output, and its comparison and repair reports. The active version cannot be dropped. Dropping
   the projection removes every version.

Checkpointed replay is defined by [Projection Replay Contracts](projection-replay-contracts.md).
Diff, comparison, and repair start with a persisted verification report and follow the
[Projection Repair Runbook](projection-repair-runbook.md).

## Activation Preconditions

Normal activation accepts a target in `built` or already `active` state whose verification state
is `verified`, `skipped`, or `unknown`. It rejects missing versions, versions still building, and
versions with failed verification. A rejected verification transition records the target,
previous version, and failure without changing the active version.

`UNSAFE` bypasses only the failed-verification guard. It does not make a missing, building, or
failed build activatable; bypass source-generation fencing; publish partial output; or make an
uncommitted metadata change visible. Use it only when an operator has independently validated the
immutable output and recorded why Cassie's verification result is being overridden.

## Failure And Retry Semantics

- Output rows and row hashes are committed in transactions of at most 1,000 rows, independent of
  the 256-row integrity range-segment size. Range and root hashes are published together only
  after all row batches finish.
- Projection-hash repair removes the root hash first, rewrites row and range hashes in
  transactions of at most 1,000 records, and publishes the root only after every batch succeeds.
- Projection metadata is published after the output root exists. Activation changes become
  visible in the in-memory catalog only after the same durable metadata commit succeeds.
- Interruption after row batches, before range/root publication, after hash publication, before
  activation publication, or before the metadata commit leaves the previous active version
  query-visible across restart.
- Source mutations durably create a maintenance-debt marker in their transaction. If post-commit
  stale marking or the later debt-detail update fails, reads fall back through the debt and source
  generation fences. Startup retries durable debt before normal service.
- A failed inactive version may be repaired with the explicit projection-version repair workflow.
  A refresh retries the active-version build. Neither retry activates an inactive version
  implicitly.

## Operator Workflow

1. Inspect `pg_catalog.pg_materialized_projections`, `pg_catalog.pg_projection_versions`,
   `pg_catalog.pg_projection_operations`, `pg_catalog.pg_projection_checkpoints`, and
   `pg_catalog.pg_maintenance_debt`.
2. Confirm source generation, freshness, checkpoint, verification, and root-hash state. Treat
   fallback diagnostics as evidence that Cassie is serving authoritative rows, not permission to
   activate an unverified version.
3. Run `VERIFY PROJECTION ... MODE full`. Use `PLAN REPAIR PROJECTION` and the repair runbook for a
   failed report; otherwise rebuild or replay from the last committed checkpoint.
4. Activate a verified built version. Retain the previous version until post-activation queries,
   metrics, and restart checks pass.
5. Roll back by reactivating a retained verified version. If no safe version remains, keep source
   fallback active and restore/rebuild from a local snapshot or authoritative source.

If `BUILD VERSION` reports that the version's output collection already exists, no version
metadata references that collection; confirm it holds nothing you need, remove it with
`DROP TABLE <output collection>`, and rerun the build.

Manual intervention is required for repeated debt retry failure, unavailable authoritative source
data, a failed full verification, exhausted query-memory limits, or recovery that cannot open the
local Midge store. Cassie does not choose another node, restore backups, or replay an external event
stream automatically.

## Capacity Evidence

Deterministic tests own exact rows, hashes, generations, checkpoints, fallback reasons, write
categories, storage reads, memory limits, worker limits, and cleanup. Tier 5 retained native-disk
evidence owns projection replay and rebuild at 10k, 100k, and 250k source rows plus verification,
repair, and restart at their registered representative scale. Fixture setup is excluded from the
measured operation. These observations are environment-labelled capacity evidence, not universal
latency guarantees. The promotion record retains the 100k refresh, replay, full-verification, and
hash-repair owners; replay throughput on hosted runners is diagnostic only, and 250k rows are not
yet retained capacity evidence. See [Production Readiness](production-readiness.md).
