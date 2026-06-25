# Phase 10 Performance Rebaseline

Report date: 2026-06-25

## Purpose

This report is the evidence surface for Phase 10 whole-system performance work. It records the local `10k` and `100k` fallback benchmark baseline, ranks bottlenecks, and assigns optimization ownership to the ordered Phase 10 issues.

Local fallback benchmark output is advisory evidence. It is not a production SLA and does not promote any feature family to production-ready by itself.

## Environment

| Field | Value |
| --- | --- |
| Profile target | `local-dev-fallback-10k`, `local-dev-fallback-100k` |
| Storage mode | `in_memory_midge_fallback` |
| Benchmark date | 2026-06-25 |
| Benchmark host | `Machine` |
| OS | Darwin 25.5.0 arm64 |
| CPU / memory | Apple M5, 10 logical CPUs, 24 GiB memory |
| Rust toolchain | `rustc 1.96.0 (ac68faa20 2026-05-25)`, `cargo 1.96.0 (30a34c682 2026-05-25)` |
| Benchmark base commit | `517069c` plus this Issue 01 harness close-out commit |
| Benchmark env overrides | `CASSIE_MIDGE_ALLOW_FALLBACK=1`; no Criterion tier overrides |

## Commands

The Phase 10 baseline used these benchmark owners:

```sh
CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo bench --locked --bench tier1_hotpath_row_codec
CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo bench --locked --bench tier1_hotpath_keys
CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo bench --locked --bench tier1_hotpath_predicates
CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo bench --locked --bench tier1_hotpath_topk
CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo bench --locked --bench tier1_hotpath_bm25
CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo bench --locked --bench tier1_hotpath_search_vector
CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo bench --locked --bench tier1_hotpath_vector_distance
CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo bench --locked --bench tier2_subsystem_sql_planning
CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo bench --locked --bench tier2_subsystem_executor
CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo bench --locked --bench tier2_subsystem_ingest
CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo bench --locked --bench tier2_subsystem_search
CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo bench --locked --bench tier2_subsystem_vector
CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo bench --locked --bench tier2_subsystem_hybrid
CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo bench --locked --bench tier3_system_query
CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo bench --locked --bench tier3_system_rebuild
CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo bench --locked --bench tier3_system_mixed_load
CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo bench --locked --bench tier3_system_concurrency
CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo bench --locked --bench tier3_system_startup
CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo bench --locked --bench tier4_integration_pgwire
CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo bench --locked --bench tier4_integration_http
```

## Scenario Evidence

| Owner | Status | Evidence |
| --- | --- | --- |
| `tier1_hotpath_*` | Completed | Row codec p95 `222 ns`, key codec p95 `719 ns`, predicate eval p95 `65.6 ns`, top-k p95 `15.3 ns`, BM25 p95 `4.31 ns`, vector distance p95 `1.85 ns` cosine / `1.15 ns` dot / `1.26 ns` L2. |
| `tier2_subsystem_sql_planning` | Completed | SQL parse p95 `8.9 us`; binding p95 `12.0 us`; logical planning p95 `12.1 us`; physical planning p95 `12.9 us`. |
| `tier2_subsystem_executor` | Completed | Simple scan p95 `38.9 us`; indexed filter p95 `11.8 us`; full-text executor p95 `196.7 us`; hybrid executor p95 `135.4 us`; vector brute force p95 `14.6 us`. |
| `tier2_subsystem_ingest` | Completed after harness fix | Write path p95 `46.9 ms`; duplicate replay p95 `121.2 ms`; lag catch-up p95 `1.071 s` at 10k and `9.428 s` at 100k. |
| `tier2_subsystem_search` | Completed with high variance | Full-text executor p95 `178.8 us` at 10k and `176.3 us` at 100k; both runs showed high outliers but emitted samples. |
| `tier2_subsystem_vector` | Completed | Vector executor p95 `14.9 us` at 10k and `246.2 us` at 100k. |
| `tier2_subsystem_hybrid` | Completed with high variance | Hybrid executor p95 `216.0 us` at 10k and `216.2 us` at 100k. |
| `tier3_system_query` | Partial with blockers | Core/scalar 10k and 100k sampled; time-series window 10k p95 `23.7 ms`; graph expand 10k p95 `7.8 us`. Graph 100k fixture setup did not emit a benchmark label after more than 6 minutes. Time-series window 100k reached warmup in an earlier run and then ran for more than 8 minutes without a sample. |
| `tier3_system_rebuild` | Partial with blocker | Projection rebuild query 10k p95 `46.3 us`. Projection refresh 10k reached warmup and Criterion estimated `2658.7 s` for 10 samples, about `265.9 s` per iteration. |
| `tier3_system_mixed_load` | Completed | Mixed ingest/query p95 `92.2 ms`; large result set p95 `89.5 us`; scaled query shape p95 `15.2 us`. |
| `tier3_system_concurrency` | Completed with high variance | Concurrent queries p50 `111.4 us`, p95 `880.8 us`. |
| `tier3_system_startup` | Completed | Cold start p95 `11.5 ms`; warm start query p95 `6.1 us`. |
| `tier4_integration_pgwire` | Completed | Simple query p95 `12.9 us` at 10k and `20.0 us` at 100k; large result set p95 `64.7 us`; concurrent connections p95 `98.5 us`. |
| `tier4_integration_http` | Completed | Document create/get p95 `19.8 ms` at 10k and `286.9 ms` at 100k; vector search p95 `107.5 ms`; concurrent requests p95 `4.4 ms`; JSON serialization p95 `119.8 us`. |

## Harness Corrections

Issue 01 allowed harness fixes only when a benchmark mutated shared state across iterations, measured the wrong path, or could not produce a local sample. The following fixes stay inside that rule:

- Replay workloads now use stable source identities per projection while keeping nonce-specific batch, event, checkpoint, and document ids.
- `tier2_subsystem_ingest` uses a replay-specific context and CSV setup load for the 100k replay fixture.
- Time-series benchmark fixtures create index/rollup/retention metadata before setup row loading, avoiding setup-time rollup refresh spill.
- Tier-3 query and rebuild owners build fixtures immediately before their owning benchmark sections instead of eagerly building every 100k fixture before the first sample.

## Blockers

| Command | Blocker | Owner |
| --- | --- | --- |
| `CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo bench --locked --bench tier3_system_query` | `time_series_window_scan/100k` reached warmup, then ran for more than 8 minutes without a completed sample. This is measured query runtime, not setup. | Issue 05 |
| `CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo bench --locked --bench tier3_system_query` | `graph_expand_query/100k` fixture setup did not emit the benchmark label after more than 6 minutes. This remains setup cost before the measured graph query. | Issue 04 |
| `CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo bench --locked --bench tier3_system_rebuild` | `projection_refresh/10k` reached warmup, then Criterion estimated `2658.7 s` for 10 samples. This is measured refresh runtime. | Issue 03 |

## Bottleneck Ranking

| Rank | Bottleneck | Evidence | Next owner |
| --- | --- | --- | --- |
| 1 | Projection refresh workflow | `projection_refresh/10k` estimated about `265.9 s` per iteration before sample completion. | Issue 03 |
| 2 | Time-series 100k window scan | `time_series_window_scan/100k` reached warmup but produced no sample after more than 8 minutes. | Issue 05 |
| 3 | Graph 100k fixture setup | `graph_expand_query/100k` did not reach its benchmark label after more than 6 minutes. | Issue 04 |
| 4 | Replay lag catch-up at 100k | p95 `9.428 s`. | Issue 03 |
| 5 | Replay lag catch-up at 10k | p95 `1.071 s`. | Issue 03 |
| 6 | HTTP document create/get at 100k | p95 `286.9 ms`. | Issue 06 |
| 7 | Duplicate replay handling | p95 `121.2 ms` despite duplicate skip semantics. | Issue 03 |
| 8 | HTTP vector search route | p95 `107.5 ms`, while direct vector executor p95 is microsecond-scale. | Issue 06 |
| 9 | Mixed ingest/query workflow | p95 `92.2 ms`. | Issue 06, with write-path dependency on Issue 03 |
| 10 | Time-series 10k window scan | p95 `23.7 ms`; 100k path is worse and blocked. | Issue 05 |

## Issue 03 Optimization Evidence

Issue 03 replay work on 2026-06-25 batched duplicate-ledger reads and avoided existing-row payload decodes when a write batch has no scalar, time-series, or graph indexes that need old values. Materialized projection rebuild writes also skip generic post-commit maintenance and rely on the explicit rebuild hash pass already required by refresh verification.

| Owner | Before | After | Result |
| --- | --- | --- | --- |
| `tier2_subsystem_ingest/projection_write_path` | p95 `46.9 ms` | p50 `36.315 ms`, p95 `44.787 ms` | Slight improvement. |
| `tier2_subsystem_ingest/projection_duplicate_replay` | p95 `121.2 ms` | p50 `117.916 ms`, p95 `121.589 ms` | No material p95 change. |
| `tier2_subsystem_ingest/projection_lag_catchup/10k` | p95 `1.071 s` | p50 `851.417 ms`, p95 `909.458 ms` | Improved. |
| `tier2_subsystem_ingest/projection_lag_catchup/100k` | p95 `9.428 s` | p50 `8.584 s`, p95 `8.703 s` | Improved but still a top write-side bottleneck. |

The diagnostic command `CASSIE_MIDGE_ALLOW_FALLBACK=1 BENCH_TIER3_WARMUP_MS=50 BENCH_TIER3_MEASUREMENT_MS=200 BENCH_TIER3_SAMPLE_SIZE=10 cargo bench --locked --bench tier3_system_rebuild -- projection_refresh/10k` still produced no completed sample after more than 90 seconds past warmup. `projection_refresh` remains the highest-priority Issue 03 blocker.

## Deferred Paths

These paths emitted local fallback samples and should not be optimized first unless later evidence changes:

- SQL parsing, binding, logical planning, and physical planning are all below `13 us` p95.
- Core point/simple reads are around `11-20 us` p95 through both system query and pgwire surfaces.
- Graph expand at 10k is `7.8 us` p95; the current graph problem is the 100k setup path, not the measured 10k traversal.
- Direct vector and full-text subsystem executors are microsecond-scale, though high outliers in 100k vector/hybrid/search should be watched after larger bottlenecks are addressed.
- Startup and pgwire are not top-ten bottlenecks in this local fallback baseline.
