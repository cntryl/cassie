# Replay workload evidence — 2026-11-16 local date

Executable source: `58e6c96ea14c3466da17c256cb2fcaefbd71f940`, tree
`d051222b580d48266b3ed604f588928bd443e660`. These are local diagnostics on
macOS arm64 using Cargo's **test profile**, two build jobs and the
`replay-benchmark-observation` feature. Under this test profile, the Cassie crate uses optimization level 1;
integration fixture code uses level 0. Other host activity was
uncontrolled. Exact versions are in [host/toolchain metadata](toolchain-host.json).

## Completed groups

| Group | Cells | Fresh stores per cell | Batch samples | Result |
| --- | ---: | ---: | ---: | --- |
| Apply: four API/embedded-SQL modes, two shapes, two batch sizes | 16 | 2 | 17,600 | 32/32 passed |
| API duplicate/no-op: two policies, two workloads, two shapes, two batch sizes | 16 | 1 | 8,800 | 16/16 passed |

Every store contains 100,000 expected rows. Batch size/count pairs are
1,000 × 100 and 100 × 1,000. Each completed control checks the independent total
row count, every title in all sixteen 6,250-row buckets, and the replay metadata
or SQL checkpoint at position 100,000, before and after clean local reopen.
Apply report counts are checked per batch; duplicate controls check every
submitted event is skipped, and no-op controls submit no events after seeding.
Seed work is excluded from measured acknowledgements and submitted/applied counts.

| Measured workload | Submitted events | Applied events | Verified duplicate skips | Calls |
| --- | ---: | ---: | ---: | ---: |
| Apply | 100,000 | 100,000 | 0 | 100 or 1,000 |
| Duplicate after seed | 100,000 | 0 | 100,000 | 100 or 1,000 |
| No-op after seed | 0 | 0 | 0 | 100 or 1,000 empty checkpoint-preserving calls |

The raw `events=100000` field is the nominal fixture budget/expected stored-row
count. It does **not** mean that no-op controls submitted 100,000 events. Use
batches per second for no-op timing; its submitted-event throughput is zero.

The groups share a **64 MiB query-memory budget and a 120,000 ms per-query
deadline**, on initial startup and reopen. This is a benchmark fixture profile,
not a production default or whole-run deadline. SQL lanes use embedded SQL
transactions; these timings do not measure pgwire/network throughput. SQL
selector controls reject duplicate/no-op workloads because those lanes have no
replay event ledger.

## Records and interpretation

- Apply [per-lane summary](summary.json) and [exact lane parameters](lanes.csv).
- Auxiliary [summary](../replay120-api-aux-2026-11-16/summary.json),
  [lane parameters](../replay120-api-aux-2026-11-16/lanes.csv) and
  [original raw receipts](../replay120-api-aux-2026-11-16/raw-receipts.tar.gz).
- [Original apply receipts](raw-receipts.tar.gz) contain every original log,
  extracted JSON record and matrix receipt. Archive metadata alone is normalized;
  [the manifest](raw-receipts-manifest.json) gives its SHA-256.
- [Structured raw apply results](raw-results.json.gz) retain every individual
  batch latency and native observation. The auxiliary directory has the same
  structured artifact. Summaries identify original per-file SHA-256 values.
- [Local gate receipts](local-qualification.json), [five-test moved example log](graph-example-output.txt),
  [fixture startup comparison](../fixture-startup-2026-11-16/fixture-profile-summary.json),
  and the [source/Jev review record](batch-a-compact-review.md).

Latency p50 is the median of individual batch elapsed times; p95 is the nearest
rank `ceil(0.95 × N)`. They are not derived by subtracting cumulative histogram
percentiles. Two apply stores and one auxiliary store per cell do not establish
population variance, stable timing quality or a production speedup.

Foreground native observations count caller-thread attempts. `primary_attempts`
means outside explicit maintenance scopes, not uniquely the owning Data commit;
`first_policy` identifies the first observed native attempt. Maintenance barrier
attempts are local-sync/cloud-strict calls inside maintenance scopes. Engine-wide
counter deltas can include background work. `wal_accounted_syncs` already includes
successful WAL fsync and ledger file/directory syncs; the fields are retained
separately and must not be added together.

In the first indexed 1,000 × 100 apply controls, strict and buffered API lanes
both recorded 300 maintenance barrier attempts and an engine-wide WAL-fsync delta
of 402. SQL default and replayable both recorded 700 maintenance barrier attempts,
300 outside-maintenance attempts and a WAL-fsync delta of 1,002; replayable made
100 of those outside-maintenance attempts buffered. Policy selection therefore
is not a barrier-free acknowledgement or observed physical-sync reduction in
those samples. Broader conclusions require their own evidence.

`recovery_ns` measures constructor/startup after **clean shutdown**, excluding
verification. These results do not establish process-kill, power-loss,
cloud-cache-loss, a bounded loss window or production qualification. Broader
[#1271](https://github.com/cntryl/cassie/issues/1271) remains separate.

## Historical 30-second profile

The [preserved original PR record](pr1332-before.json) belongs to older source
`c5b59295`: thirteen large controls passed, one unindexed SQL-default title-10
verification query failed with `57014`, and two controls were not run. It is
archived unchanged, including its then-current status and plans. Those results
are not combined with the new profile or relabeled passing.

## Reproduction

Use a clean isolated checkout at the executable source above, pinned Cargo.lock,
and the recorded toolchain/profile. For every row of each group's `lanes.csv`,
run one fresh process with its exact mode, shape, workload, batch size and count:

```sh
CARGO_BUILD_JOBS=2 CASSIE_REPLAY_BENCH_SOURCE='58e6c96ea14c3466da17c256cb2fcaefbd71f940;tree=d051222b580d48266b3ed604f588928bd443e660;profile=test;feature=replay-benchmark-observation' CASSIE_REPLAY_BENCH_MODE=<mode> CASSIE_REPLAY_BENCH_SHAPE=<shape> CASSIE_REPLAY_BENCH_WORKLOAD=<workload> CASSIE_REPLAY_BENCH_BATCH_SIZE=<batch_size> CASSIE_REPLAY_BENCH_BATCHES=<batch_count> cargo bench --locked --features replay-benchmark-observation --bench tier3_replay_durability --profile test
```

The source label is operator-provided; verify it against Git. `repeat` identifies
fresh processes, not reused stores. Original storage paths are diagnostic
identities; generated stores and input data are not committed.
