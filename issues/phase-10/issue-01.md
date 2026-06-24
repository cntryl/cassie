# Phase 10 Issue 01: Baseline Evidence And Bottleneck Ranking

## Status

Open.

## Goal

Record a whole-system performance baseline for the existing `10k` and `100k` local fallback profiles, then rank the bottlenecks that later Phase 10 issues are allowed to optimize.

## Dependencies

- Phase 04 through Phase 09 are closed.
- `issues/phase-00/issue-01.md` points to this issue first.
- No code optimization work from later Phase 10 issues has started.

## Implementation Plan

1. Create `docs/performance-rebaseline-phase-10.md` with sections for benchmark environment, commands, scenario results, bottleneck ranking, and deferred paths.
2. Audit benchmark owners before collecting evidence. Fix benchmark-harness bugs only when the existing harness mutates shared state across iterations, measures cleanup/setup in the timed section unintentionally, or cannot produce a local sample with the documented default configuration.
3. Run `cargo bench --locked --bench <bench>` for the whole-system baseline owners:
   - `tier1_hotpath_row_codec`
   - `tier1_hotpath_keys`
   - `tier1_hotpath_predicates`
   - `tier1_hotpath_topk`
   - `tier1_hotpath_bm25`
   - `tier1_hotpath_search_vector`
   - `tier1_hotpath_vector_distance`
   - `tier2_subsystem_sql_planning`
   - `tier2_subsystem_executor`
   - `tier2_subsystem_ingest`
   - `tier2_subsystem_search`
   - `tier2_subsystem_vector`
   - `tier2_subsystem_hybrid`
   - `tier3_system_query`
   - `tier3_system_rebuild`
   - `tier3_system_mixed_load`
   - `tier3_system_concurrency`
   - `tier3_system_startup`
   - `tier4_integration_pgwire`
   - `tier4_integration_http`
4. Use existing Criterion JSON/output and benchmark console output to fill the report. If a benchmark cannot run locally, record the exact command, failure, and whether it blocks optimization for its family.
5. Rank the top ten bottlenecks by p95/p99 impact, fallback/access-path risk, and cross-feature blast radius.
6. List paths that should not be optimized in Phase 10 because they did not miss a contract or lack enough evidence.

## Acceptance Criteria

- `docs/performance-rebaseline-phase-10.md` includes environment metadata, commands run, and a table of benchmark evidence.
- Benchmark owners used for the baseline do not drift into a different measured workload after the first iteration.
- The report names the exact next issue owner for each ranked bottleneck.
- No implementation optimization is included in this issue.
- The phase backlog remains ordered through `issues/phase-00/issue-01.md`.

## Validation

Run in order:

```sh
cargo build --locked --bin cassie
CASSIE_MIDGE_ALLOW_FALLBACK=1 cargo test --locked
cargo fmt --all -- --check
```

No `cntryl-tools validate-tests` command is required unless a test file is touched.

## Close-Out

- Update `issues/phase-10/README.md` with a short archived summary of the baseline.
- Delete this issue file only after the report and validation are complete.
- Update `issues/phase-00/issue-01.md` so the next open issue is `issues/phase-10/issue-02.md`.
- Commit the completed slice.
