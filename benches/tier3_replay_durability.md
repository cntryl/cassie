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

## Separate duplicate/no-op lanes and clean recovery

`CASSIE_REPLAY_BENCH_WORKLOAD` selects `apply` (default), `duplicate` or `noop`.
The latter two are replay-API-only lanes; SQL modes reject them before opening a
store, since the SQL fixture has no event ledger. Both seed the full fixture
before timing. `duplicate` resubmits the same event ids and checks that every
one is skipped. `noop` submits an empty event list per batch: it measures a
metadata/checkpoint-preserving acknowledgement, not an optimized zero-I/O path.
Both retain the seeded rows and final checkpoint. Setup commits are excluded
from the measured counter window, though background work can cross that window.

Each result identifies the workload and selected primary policy. `events` is
the fixture size; `submitted_events` excludes seed events and is zero for `noop`;
`applied_events` is zero for duplicate and no-op lanes. `events_per_second` uses
submitted events, so no-op throughput should be calculated as batches/time.
The per-batch samples retain full acknowledgement latency for all lanes.

After recording measurement counters, the driver verifies row count, all 16
title equality buckets, and the replay metadata or SQL checkpoint. Indexed
fixtures exercise the planner-selected equality read path; this does not force
or independently attest an index plan. It then shuts down cleanly, releases the
engine, reopens the same directory and checks the same results again.
`recovery_ns` measures constructor plus startup after **clean shutdown**, with
verification excluded. It is not crash recovery, process-kill, power-loss or
cloud-cache-loss latency. `verification_ns` is the first untimed verification.
Neither interval enters acknowledgement samples or measured native counters.

The additions are authored, not executed, until the assembler's frozen gate.
#1271 remains open: repeated 100k-row measurements across profiles, shapes and
batch sizes, crash recovery time, power-loss and cloud-cache-loss qualification,
and production operator diagnostics are not established by adding these lanes.
Existing #1271 historical measurements at `a9231cd09` remain a separate source
identity. No speedup or bounded-loss claim follows from the driver.

## Explicit analytical query-memory profile

The persistent replay driver selects a **64 MiB query-memory budget** for both
its initial engine and clean reopen, matching the existing analytical benchmark
profile in `benches/support/workloads/context.rs`. The untimed full-row/count
verification of the first 100k-row indexed strict run exceeded the ordinary
10 MiB default (`10486558 > 10485760`); that failed run remains failed evidence.
The selected repair changes only the benchmark fixture configuration, retains
all input rows and verification assertions, and records
`query_memory_budget_bytes` from the exact configuration used for both opens.
Production defaults and write durability policies are unchanged. Source-backed
Jev selected the existing explicit 64 MiB analytical profile at probability
1.00; successful corrected runs must be recorded against their own source.

### Bounded title verification

The diagnostic run at `e88dd5d0` identified the 64 MiB failure as
`SELECT COUNT(*) FROM replay_bench WHERE title = 'title-0'` (`54000`);
the independent unfiltered total count had passed. The typed aggregate predicate
supports numeric/boolean fields, not text (`aggregate_exec/typed/predicate.rs`),
so this query falls back to retaining the full source before filtering. This
is an existing execution boundary, not a repaired engine path.

The selected verifier keeps the independent unfiltered total count and replaces
each text-filtered aggregate with `SELECT title ... WHERE title = 'title-N'`.
The projected read streams admitted input batches, filters before retaining
output, and retains only that bucket's matching title rows. Each bucket asserts
its full expected cardinality and every returned title, then releases the result
before querying the next bucket. The independent total count still detects
extra rows with unknown or null titles; the total is never inferred by summing
known buckets. All checkpoint and clean-reopen checks and the 64 MiB budget stay
in place. Jev selected this finite verifier change at probability 0.98; its
0.36 oracle-gap lead requires focused short/extra/wrong/null-result controls
and actual 100k-row execution. Authored controls are not passing evidence.

## Bounded replay verification deadline

This driver now selects a **replay-specific 120,000 ms query deadline** together
with its 64 MiB query-memory budget, for both initial startup and clean reopen.
The deadline is finite and applies to each SQL query; it is not a limit on the
whole benchmark run. The 120-second cap matches the existing large analytical
benchmark timeout value, but this is a new replay benchmark profile, **not** a
pre-existing 100k-row representative default or a production-default change.
The JSON result records `query_timeout_ms` from the actual configuration used
for both engines, alongside `query_memory_budget_bytes`.

The earlier 30-second profile remains historical evidence: thirteen large
controls passed, an unindexed SQL-default apply run failed during untimed
verification of `title-10` with `57014`, and the final two controls were not run.
The failure remains a failed query at that profile, not proof of a production
runtime defect or a passing measurement. The selected finite next step is to
rerun the failed control and the two remaining controls, plus needed targeted
smoke coverage, on the new source with this bounded 120-second profile. Jev
selected that step at probability 0.76; completion is subject to actual results.

Old 30-second successes and new 120-second runs must retain separate source and
profile identities. Combining them does not establish a uniform matched 100k
matrix or the repeated comparisons required by #1271. Uniform-profile reruns,
repeated measurements and broader durability qualification remain follow-up
work. The fixture keeps all rows, title-bucket, total-count, checkpoint and
reopen assertions; this profile change neither weakens them nor raises the
memory budget. No global environment variable or production deadline is changed.
