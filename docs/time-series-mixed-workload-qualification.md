# Time-series mixed workload qualification (#1350)

This page records the harness, its deterministic correctness verdict, the run
command and the evidence. It records no throughput or latency result. The
machine used for development is never quiet, so every rate or latency number a
smoke run prints is not-quiet evidence and decides nothing.

## Harness

`benches/tier5_time_series_mixed.rs` drives `benches/support/workloads/time_series_mixed*`
on a native-disk store with a fixed one-day time-series index partitioned by
device: one writer with 128-event checkpointed batches, four prepared range
readers, and one maintenance worker. A mixed run executes a full rollup refresh,
a 2% out-of-order correction burst, and explicit scheduled retention.

## Correctness verdict (deterministic)

The verdict is a set of exact counts and exact model comparisons. Nothing in it
depends on how fast the machine ran.

| Criterion | How it is decided |
|---|---|
| Reader pages | A synchronized probe opens a repeatable-read snapshot, reads the projection checkpoint inside it, runs sampled prepared pages, and compares them with the independent event model replayed through exactly that checkpoint. |
| Backfill | When the whole correction burst is acknowledged (counter equality with the 2% expected count), a full-relation probe compares every row against the model at its snapshot's checkpoint. |
| Retention | Retention starts only after the refresh, the correction drain and their probes are complete, so the model's retention state is known. After it, the policy-reported deleted rows must equal the model's expired-row count, no expired row may remain, and a page probe and the final full-relation comparison run against the retained model. |
| Ingest accounting | Every offered event is acknowledged, and the final relation and grouped rollups equal the model. |
| Phase release | The contract minutes are release times. A phase never starts early, and a phase the measured window did not reach runs to completion afterwards. There is no completion deadline; the hang guards abort a wedged run and never decide a verdict. |

## Timing criteria that stay open

These are recorded with raw samples and printed as open, and they are not part of
the verdict:

- steady-ingest backlog stable through the window (the correction burst is
  accounted separately and excluded from this backlog);
- an ingest-only run finds a sustainable rate, and mixed ingest at 50% of it;
- mixed range-read p95 within 2x reader-only p95.

They stay open because they are throughput and latency claims. The owner rule is
that there is no quiet-machine measurement campaign on the development machine,
so a decision needs a quiet qualification host.

## Smoke findings and their causes

| Finding | Cause | Resolution |
|---|---|---|
| Oracle page mismatch | Harness. A probe requested when the refresh finished ran while retention was already deleting rows, but the model assumed retention had not run. | Retention is released only after the refresh, correction drain and earlier probes finish. |
| Retention reported complete with old rows present | Harness. The progress lookup used the unqualified policy name, found no row, and read that as "no pending cutoff". The lookup now matches the qualified name and a missing row aborts. | Fixed in the harness; progress is checked by scanned-row counters. |
| Retention not completing in 180 s | Harness. The retention loop had a 180 s deadline. Retention scans 256 rows per batch in id order, so a 100k-row table needs about 400 batches. | The deadline is gone. Each batch must advance the scanned-row counter or the run aborts, and the deleted-row count is checked exactly. |
| Backlog above the stability bound | Harness accounting. The 2,000-event correction burst and the writer quiesced for the refresh counted as steady backlog. | The burst is accounted separately; stability is an open timing criterion. |

No engine defect was found in the retention, refresh or snapshot paths by this
pass. These engine observations are not defects: a full rollup refresh does not publish while the source commits
(by design, the harness quiesces the writer), a full relation read costs about
5.5 KB of query memory per row, time-series pages are not limited early, and
`event_at::TEXT` skips the time-series path without a fallback counter. One
retention batch of 256 scanned rows took on the order of a second at 100k rows on
the loaded machine; that is a latency observation and is open.

## Run command

Qualification run (the full matrix; requires the Apple M5 host profile):

```sh
CASSIE_BENCH_DEPLOYMENT_PROFILE_ID=workstation-apple-m5-arm64-apfs \
  cargo bench --locked --bench tier5_time_series_mixed
```

Harness smoke run, the deterministic correctness verdict at a small fixed seed:

```sh
CASSIE_BENCH_DEPLOYMENT_PROFILE_ID=workstation-apple-m5-arm64-apfs \
CASSIE_BENCH_TS_MIXED_SMOKE_EVENTS=20000 \
CASSIE_BENCH_TS_MIXED_ONLY=smoke:16 \
CASSIE_BENCH_TS_MIXED_TIME_SCALE_PERCENT=5 \
CASSIE_BENCH_TS_MIXED_SMOKE_MIXED_ONLY_RATE=512 \
  cargo bench --locked --bench tier5_time_series_mixed
```

`CASSIE_BENCH_TS_MIXED_SMOKE_MIXED_ONLY_RATE` pins the ingest-only point so only
the mixed scenario runs. The run prints `correctness verdict: PASS` and each open
timing criterion. The stress runner reports its own sample-count quality row as
invalid for one sample; that row is a measurement-quality note, not a correctness
result.

## Evidence

Raw JSON for each scenario is written under `target/time-series-mixed-1350/`.

Clean smoke run (20,000 seed events, 16 devices, 5% time scale, loaded machine, so
all rates are not-quiet and decide nothing): `correctness verdict: PASS`. The run
acknowledged 40,592 of 40,592 offered events, drained all 400 of 400 expected
correction events, ran 4 snapshot probes (3 page probes and 1 full-relation
probe) with 0 mismatches, and retention ran 205 batches, reported 10,232 deleted
rows against 10,232 expected, with 0 expired rows remaining. The one open timing
criterion printed was the steady-ingest backlog stability. The 100k-seed smoke
configuration was not rerun on the final harness; the pre-fix 100k run is the
failing baseline recorded above.
