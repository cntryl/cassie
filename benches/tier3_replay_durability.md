# Local replay durability evidence

This driver measures newly applied strict or buffered replay batches on fresh,
independent local stores. It creates the same row table and scalar index,
prepares all inputs before measurement, and keeps postcommit maintenance
in the timed acknowledgement. The default profile is `local_strict`; choose
`local_buffered` explicitly to exercise the replayable API. Each batch asserts
that the first native policy matches its selected profile. This first-attempt
check is meaningful for this fixture: startup and table/index creation finish
before observation, and valid simple replay performs only reads until its
primary commit. It does not attribute later maintenance or background syncs.

With the `replay-benchmark-observation` feature enabled, run both profiles with
matching batch sizes and counts, and label each output with the exact source and
instrumentation commits:

```sh
CASSIE_REPLAY_BENCH_SOURCE='<source-and-instrumentation-commits>' CASSIE_REPLAY_BENCH_MODE=local_strict CASSIE_REPLAY_BENCH_BATCH_SIZE=64 CASSIE_REPLAY_BENCH_BATCHES=32 cargo bench --locked --features replay-benchmark-observation --bench tier3_replay_durability
CASSIE_REPLAY_BENCH_SOURCE='<source-and-instrumentation-commits>' CASSIE_REPLAY_BENCH_MODE=local_buffered CASSIE_REPLAY_BENCH_BATCH_SIZE=64 CASSIE_REPLAY_BENCH_BATCHES=32 cargo bench --locked --features replay-benchmark-observation --bench tier3_replay_durability
```

The mode must be one of those two exact values; invalid or non-Unicode values
fail before a store is opened. Output identifies the selected mode and replay
API. Use the same Midge release, machine and build settings, alternate or
otherwise control run order, repeat runs, and record the host. The source label
is provided by the operator and must be verified against Git; it is not
automatically attested by the driver. The historical #1270 comparison used the
then-strict `replay_projection_batch` API on source versions before the
explicit strict method existed; record that source difference when comparing.

Repeat with additional batch sizes and retain the raw JSON result for every run.
Each JSON row contains setup and measurement durations, event throughput, batch
acknowledgement latency samples, per-batch caller-thread native commit outcomes,
and Midge engine-wide counters before and after the measurement window. Compute
counter deltas and batch latency distributions from the raw samples. A native
commit's success is not necessarily a successful Cassie acknowledgement if a
later operation fails. Observation excludes other threads and counts native
commit calls, not attempts to enter publication, overlay staging or suppressed
commits. The benchmark feature is absent from normal builds.

Midge `wal_fsync_count` counts successful physical WAL writer syncs and a sync
can satisfy multiple requests. `wal_accounted_syncs` adds successful ledger
file and directory syncs, but excludes segment initialization/repair and other
filesystem syncs. Counter deltas are engine-wide and may include background
activity; they are not exact replay-attributable counts. The fixture deliberately
does not assert timings or equate one commit with one physical sync.

The unique temporary data directory is printed for inspection and is not removed
by the driver. Explicitly manage benchmark artifacts and disk capacity. This
experiment is a local smoke comparison, not a buffered crash qualification,
cloud-loss or power-failure proof, or a general speedup across workloads. The
full 100k-row, indexed/unindexed and SQL comparison remains in #1271.
Performance comparisons include all differences between source versions and
observation overhead; they cannot by themselves attribute a
change solely to the removed commit barrier.

## Shapes, SQL projector lanes and commit attribution

`CASSIE_REPLAY_BENCH_SHAPE` selects `indexed` (default) or `unindexed`; the
unindexed fixture omits the scalar index so the two shapes can be compared on
matched stores. `CASSIE_REPLAY_BENCH_MODE` also accepts `sql_default` and
`sql_replayable`. Those lanes run one explicit SQL transaction per batch that
inserts the batch's rows and updates a single-row checkpoint table; the
replayable lane first runs `SET LOCAL cassie.projection_write_mode =
'replayable'` (#1279). For SQL lanes the timed acknowledgement covers the whole
transaction, while native commits are observed around `COMMIT` only, so earlier
statement work is in the latency sample but not in the commit counts. A SQL
checkpoint table has no replay event ledger; these lanes do not exercise
duplicate detection.

Each sample's `foreground_native_commits` attributes caller-thread commit
attempts to one source: `primary_attempts` (outside any maintenance scope),
`column_maintenance_attempts`, `hash_maintenance_attempts` and
`time_series_maintenance_attempts`. `maintenance_barrier_attempts` counts the
local-sync or cloud-strict attempts made inside a maintenance scope (#1277).
These are call attempts on the observing thread. They are not physical syncs,
they exclude background threads and staged or suppressed commits, and one
physical WAL sync can satisfy several of them.

No run of these lanes is recorded here. The 100k-row matched comparison across
batch sizes, the duplicate and no-op cases, recovery time, and any cloud
profile remain open under #1271.
