# Phase 10 Performance Rebaseline

Report date: 2026-06-24

## Purpose

This report is the evidence surface for Phase 10 whole-system performance work. It records the local `10k` and `100k` fallback benchmark baseline, ranks bottlenecks, and assigns optimization ownership to the ordered Phase 10 issues.

Local fallback benchmark output is advisory evidence. It is not a production SLA and does not promote any feature family to production-ready by itself.

## Environment

| Field | Value |
| --- | --- |
| Profile target | `local-dev-fallback-10k`, `local-dev-fallback-100k` |
| Storage mode | `in_memory_midge_fallback` |
| Benchmark date | 2026-06-24 |
| Benchmark host | Pending |
| Commit | Pending |
| Rust toolchain | Pending |
| Benchmark env overrides | None unless recorded per command |

## Commands

The Phase 10 baseline uses these benchmark owners:

```sh
cargo bench --locked --bench tier1_hotpath_row_codec
cargo bench --locked --bench tier1_hotpath_keys
cargo bench --locked --bench tier1_hotpath_predicates
cargo bench --locked --bench tier1_hotpath_topk
cargo bench --locked --bench tier1_hotpath_bm25
cargo bench --locked --bench tier1_hotpath_search_vector
cargo bench --locked --bench tier1_hotpath_vector_distance
cargo bench --locked --bench tier2_subsystem_sql_planning
cargo bench --locked --bench tier2_subsystem_executor
cargo bench --locked --bench tier2_subsystem_ingest
cargo bench --locked --bench tier2_subsystem_search
cargo bench --locked --bench tier2_subsystem_vector
cargo bench --locked --bench tier2_subsystem_hybrid
cargo bench --locked --bench tier3_system_query
cargo bench --locked --bench tier3_system_rebuild
cargo bench --locked --bench tier3_system_mixed_load
cargo bench --locked --bench tier3_system_concurrency
cargo bench --locked --bench tier3_system_startup
cargo bench --locked --bench tier4_integration_pgwire
cargo bench --locked --bench tier4_integration_http
```

## Scenario Evidence

Partial benchmark execution started on 2026-06-24 and exposed benchmark-harness issues before a complete whole-system baseline could be trusted.

| Owner | Status | Evidence |
| --- | --- | --- |
| `tier1_hotpath_*` through `tier2_subsystem_sql_planning` | Completed before harness audit | Criterion samples exist under `target/criterion`; do not use as final Phase 10 evidence until the full baseline is rerun from the corrected harness. |
| `tier2_subsystem_executor` | Completed before harness audit | Criterion samples exist under `target/criterion`; do not use as final Phase 10 evidence until the full baseline is rerun from the corrected harness. |
| `tier2_subsystem_ingest` | Targeted smoke sample collected | The original write benchmark spent more than 9 minutes without producing a sample. Stack sampling showed the run was still building the benchmark fixture, not measuring replay. After the CSV setup-load path and replay source-identity correction, `CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo bench --locked --bench tier2_subsystem_ingest -- projection_lag_catchup/100k` collected 10 samples: [7.9968 s, 8.0197 s, 8.0470 s]. |
| `tier2_subsystem_search` | Interrupted before first sample | The run was stopped after the ingest blocker was identified. Search 100k setup still needs the same setup/measurement audit before final evidence. |

## Benchmark Harness Audit

Findings from the interrupted baseline run:

- `projection_write_path` used `iter_custom` but reported only ingest elapsed time while cleanup still consumed wall-clock time. Criterion could therefore schedule too many iterations and delay sample output.
- `projection_duplicate_replay` and `projection_lag_catchup` reused fixed replay event ids, checkpoints, batch ids, and document ids. After the first iteration, the measured workload drifted into duplicate replay rather than the intended apply/skip or catch-up path.
- `projection_lag_catchup` changed source identities between measured iterations for the same projection, which violated the projection/source binding contract after the first successful replay.
- `time_series_retention_enforcement` and `time_series_rollup_refresh` reused fixed document ids, so later iterations overwrote the same row instead of measuring a fresh mutation.
- The ingest 100k fixture setup is too expensive to use blindly in the baseline run. Stack sampling showed CPU in `Midge::put_documents` through `delete_normalized_vector_keys_for_document` before any Criterion sample was emitted. This is setup work, but it blocks evidence collection and should be separated from the measured replay contract.

Harness corrections started in this slice:

- Write and HTTP timed batch helpers now report elapsed time after cleanup, so `iter_custom` accounting matches wall-clock work.
- Replay workloads now take a nonce and generate per-iteration event ids, checkpoints, batch ids, source identities, and document ids.
- `projection_lag_catchup` now keeps one stable source identity per projection while preserving nonce-specific batches and events.
- Time-series mutable workloads now write nonce-specific document ids.
- `tier2_subsystem_ingest` now uses a replay-specific context and pgwire-equivalent CSV bulk load for the 100k replay fixture so unrelated vector payloads and per-row setup writes do not dominate replay setup.

Remaining before final baseline:

- Re-run the full `tier2_subsystem_ingest` owner after the targeted smoke correction to record final owner-level sample evidence.
- Audit `tier2_subsystem_search`, `tier2_subsystem_vector`, `tier2_subsystem_hybrid`, and the tier-3/tier-4 owners for expensive 100k setup that blocks local evidence before any Criterion sample appears.
- Do not rank performance bottlenecks until the full corrected baseline can emit samples for every required owner or records an explicit per-owner blocker.

## Bottleneck Ranking

Blocked until benchmark-harness setup issues are resolved and the full baseline can be rerun.

## Deferred Paths

Blocked until benchmark-harness setup issues are resolved and the full baseline can be rerun.
