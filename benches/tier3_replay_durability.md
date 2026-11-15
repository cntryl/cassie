# Local replay durability evidence

This driver compares a fixed number of newly applied replay batches on fresh,
independent local stores. It creates the same row table and scalar index in each
run, prepares all inputs before measurement, and uses the replay API's local
synchronous write policy. Postcommit maintenance remains enabled. Run the
original two-transaction source and the atomic source with the same benchmark
and observation module, the same Midge release, batch sizes and event count,
and the same machine and build settings. Alternate or otherwise control run
order, repeat runs, and record host, build, source commit and instrumentation
commit identities. The source label is provided by the operator and must be
verified against Git; it is not automatically attested by the driver.

Once the assembler has registered the `replay-benchmark-observation` feature
and this bench target in each comparison checkout, a representative invocation is:

```sh
CASSIE_REPLAY_BENCH_SOURCE='<source-and-instrumentation-commits>' CASSIE_REPLAY_BENCH_BATCH_SIZE=64 CASSIE_REPLAY_BENCH_BATCHES=32 cargo bench --locked --features replay-benchmark-observation --bench tier3_replay_durability
```

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
experiment does not qualify buffered WAL, cloud loss, power failure, or a general
speedup across workloads. Performance comparisons include all differences between
source versions and observation overhead; they cannot by themselves attribute a
change solely to the removed commit barrier.
