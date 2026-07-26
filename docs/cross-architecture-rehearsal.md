# Cross-Architecture Release Rehearsal

This runbook defines the evidence record for the supported `linux/amd64` and `linux/arm64`
container paths. It is an operator rehearsal checklist, not a claim that either architecture
has been exercised by this checkout. Record one evidence bundle per image digest, architecture,
deployment profile, and release commit.

## Required identity

Record the release commit, image digest, Cassie and Midge revisions, architecture, runner,
kernel, filesystem, deployment profile, fixture size, operator, UTC start/end times, and the
exact commands. Do not combine amd64 and arm64 results into one artifact.

The existing [Containers workflow](../.github/workflows/containers.yml) builds both release
architectures. Run this rehearsal only after resolving the immutable image digest from that
workflow and recording the digest in the evidence bundle.

## Rehearsal sequence

Run the following sequence independently on each architecture and profile. Use an empty host
directory for each attempt and retain the command output and container logs.

```sh
export IMAGE='ghcr.io/cntryl/cassie@sha256:<immutable-digest>'
export DATA_DIR="$PWD/rehearsal-data/<architecture>"
export SNAPSHOT_DIR="$DATA_DIR/snapshots"
export RESTORE_DIR="$DATA_DIR/restore"
export CASSIE_ADMIN_PASSWORD='<non-default-rehearsal-secret>'

mkdir -p "$DATA_DIR" "$SNAPSHOT_DIR" "$RESTORE_DIR"
docker run --rm --name cassie-rehearsal \
  -p 127.0.0.1:18080:8080 -p 127.0.0.1:15432:5432 \
  -e CASSIE_REST_LISTEN=0.0.0.0:8080 \
  -e CASSIE_PGWIRE_LISTEN=0.0.0.0:5432 \
  -e CASSIE_ADMIN_PASSWORD \
  -e CASSIE_MIDGE_DATA_DIR=/data/midge \
  -e CASSIE_ALLOW_INSECURE_NON_LOOPBACK_LISTEN=1 \
  -v "$DATA_DIR:/data" "$IMAGE"
```

Use a unique non-default value for `CASSIE_ADMIN_PASSWORD` and keep it out of retained
command logs. The host paths in `DATA_DIR`, `SNAPSHOT_DIR`, and `RESTORE_DIR` are mounted
under `/data`, so use `/data/snapshots` and `/data/restore` from inside the container.

While the container is running, record successful health/readiness responses, authenticated
pgwire and REST queries, a representative write/read result, `/metrics`, and `EXPLAIN` output.
Restart the same container and verify the same result plus catalog and derived-state health.

Exercise snapshot and restore to a new directory using the documented
[Snapshot And Restore](snapshot-restore.md) API. After restore, start a fresh container from
the restored directory and verify row results, indexes, projections, catalog state, and the
absence of partial sidecars before serving traffic.

Exercise rebuild and repair using the documented [Projection Repair Runbook](projection-repair-runbook.md):

1. Capture `VERIFY PROJECTION ... MODE full` and the pre-repair integrity report.
2. Run `PLAN REPAIR ...` and stop if `executable = false`.
3. Execute the smallest required repair scope.
4. Verify again and retain the repair report, post-verification state, metrics, and query results.

For failure injection, interrupt startup, snapshot copy, restore copy, and a derived-state
publication at the documented test seam. Confirm the operation fails with its expected
diagnostic, removes partial output, leaves authoritative rows queryable, and does not publish
stale or incomplete derived state after restart. Record the rollback command and the exact
observed error for every injection.

## Evidence record

The retained bundle must contain:

- architecture and profile identity, image digest, commit, toolchain, filesystem, and fixture;
- startup, health, authenticated query, restart, snapshot, restore, rebuild, repair, and
  failure-injection logs with elapsed times;
- `/metrics`, `EXPLAIN`, integrity, repair, and catalog output before and after recovery;
- expected versus observed diagnostics, query results, sidecar state, and partial-output checks;
- rollback command, operator decision, UTC timestamps, and links to the image/build artifacts.

Missing evidence keeps that architecture/profile rehearsal incomplete. A successful local test,
single-architecture container build, or image manifest inspection does not satisfy this runbook.
