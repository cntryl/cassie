# Performance Contracts

This document is the canonical owner for query-pattern access paths, bounded-resource behavior, read and write amplification, and benchmark evidence. It defines machine-independent contracts. Latency observations are comparative and always labelled with their environment.

## General Contract

Every performance-sensitive query family must prove:

1. exact result cardinality and semantics against a reference path;
2. the selected physical access path and any fallback reason;
3. bounded storage reads, candidate rows, result rows, memory, and workers;
4. cancellation and deadline checks inside long-running work;
5. no execution-result-cache hits unless the benchmark explicitly owns cache behavior;
6. one complete artifact from a single commit, toolchain, profile, and fixture definition.

Missing access-path evidence is a failed contract, not permission to infer the intended path from latency.

## Query Memory and Results

One shared tracker accounts query-owned materialization: sorts, distinct sets, hash tables, aggregate state, recursive working sets, graph frontiers, search candidates, portal buffers, and execution-result-cache payloads. Operators reserve before allocation and release on drop. Exceeding the configured query budget returns SQLSTATE `54000`.

`CASSIE_QUERY_MEMORY_BUDGET_BYTES` configures the per-query accounted-memory budget (default `10485760`); the unreleased temporary-spill name is not accepted. `CASSIE_MAX_RESULT_ROWS` configures the result-row cap (default `100000`). Cassie does not promise disk spilling: operators either stay within accounted memory or fail with `54000`.

Execution-result caching is configured by `CASSIE_EXECUTION_RESULT_CACHE_ENABLED` (default `true`), `CASSIE_EXECUTION_RESULT_CACHE_MAX_ENTRIES` (default `64`), and `CASSIE_EXECUTION_RESULT_CACHE_MAX_BYTES` (default `67108864`). Eligibility is decided from the resolved physical plan. Active transactions, virtual catalogs, provider-backed work, and non-immutable user functions bypass the cache. Safe keys include normalized user, database, search path, parameters, execution mode, schema epoch, and data epoch.

Streaming scan, filter, projection, limit, scoring, and eligible aggregation paths must keep memory proportional to batch size or the requested result window. Blocking operators may materialize only accounted state. Result-row limits are enforced while producing rows. Embedded APIs may materialize the final bounded result; pgwire portals retain resumable execution state. A portal's result-row limit is cumulative across resumes, and retained portal memory is charged cumulatively across all live portals on the connection. A resume or bind that would exceed either limit fails with `54000` without publishing a partial page; close, rollback, and disconnect release the retained state.

SQL parsing rejects text over 1 MiB, more than 100,000 lexical tokens, nesting deeper than 128, nested block comments deeper than 128, and pgwire simple-query batches over 256 statements. These failures are HTTP `400` or SQLSTATE `54000`. Pgwire emits row descriptions, rows, and command completion incrementally, caps each backend frame at 16 MiB, and retains the generic 16 MiB frontend-frame cap for non-SQL bind and COPY data. Admin UI files retain their 8 MiB cap and stream in chunks no larger than 64 KiB.

## Relational Access Paths

| Query pattern | Required path evidence | Bound |
| --- | --- | --- |
| Primary-key equality | Point lookup | Constant key reads plus row decode |
| Scalar equality or range | Ordered index prefix/range scan and candidate row fetch | Reads proportional to matching candidates and requested window |
| Covered lookup | Covering index decode without base-row fetch | Index entries visited |
| Ordered page | Order-compatible index scan | Page size plus bounded predicate skips |
| Keyset page | Exclusive continuation bound | Page size plus bounded predicate skips |
| `LIMIT` or `EXISTS` | Pull termination | No reads after the requested result is proven |
| Count or aggregate | Streaming or equivalence-proven aggregate/column path | Batch memory or documented aggregate state |
| Join | Named algorithm, legal order, estimates, build/probe bounds | Accounted build state and bounded workers |

`EXPLAIN` must identify scans, ordering, filters, estimates, join order and algorithm, legality barriers, fallback reasons, and memory bounds relevant to the selected tree.

Inner-join planning exhaustively enumerates deterministic relation orders through eight relations and uses deterministic greedy expansion above eight. Outer, full, cross, lateral, and correlated dependencies remain explicit legality barriers unless a semantics-preserving proof applies. Missing statistics use a stable conservative fallback and lexical tie-breaking, so identical schema and statistics snapshots produce identical plans and reusable plan-cache entries.

## Retrieval Access Paths

Full-text indexed execution reads persisted posting blocks and document statistics, computes exact BM25 scores, maintains a bounded result window, renders snippets from fetched candidates, and fetches only candidate rows. Eligible scalar equality indexes are intersected before row fetch. Transaction overlays and missing, stale, corrupt, or incomplete artifacts use an explicitly labelled row fallback under the same cancellation and memory controls.

Exact vector search reads lazy Midge cursor batches and retains only a memory-accounted top-k heap. HNSW reads persisted node records; IVFFlat reads persisted membership prefixes. Approximate paths expand candidates deterministically within the configured cap and exact-rerank selected source rows. Each ANN candidate batch carries its persisted source generation, which is fenced before, during, and after reranking. A missing row, malformed or dimension-invalid vector, or generation change labels the attempt `concurrent-source-change`, discards all attempted-path rows and metrics, and executes the exact controlled path once. Structured filters and transaction overlays use an explicitly diagnosed exact fallback; candidate exhaustion produces an exact fallback or resource error rather than silent truncation.

Hybrid retrieval combines persisted text, vector, and structured candidates before exact final scoring under the shared query memory and cancellation controls. It reports component candidate counts, final row fetches, and fallback reasons.

Remote embedding providers expose controlled document and query methods. Each request, retry, and backoff observes cancellation and clamps its transport timeout to the remaining query deadline. Provider success and error bodies are bounded by `CASSIE_EMBEDDINGS_MAX_RESPONSE_BYTES` (default 8 MiB), declared oversized bodies are rejected before reading, chunked bodies stop at the limit plus one byte, and propagated provider-error excerpts are control-free and capped at 1 KiB. Stable describes Cassie's protocol behavior and deterministic local contract evidence, not the availability or latency of third-party services.

## Time-Series, Graph, and Column Batches

- Time-series queries use ordered partition/timestamp bounds and point-fetch candidate rows. Unsupported shapes use a labelled row fallback.
- Graph traversal reads controlled pages from edge-type-first prefixes when filtered and weight-first node prefixes when unfiltered. Both-direction scans merge by weight and edge ID. Frontier, visited, path, edge, and output state are accounted before retention; `54000` returns no partial traversal. Transaction overlays, `missing-sidecar-manifest`, `sidecar-format-mismatch`, `malformed-sidecar`, and `concurrent-source-change` use the exact session-aware row path.
- Column-batch execution uses typed vectors, validity and selection vectors, segment summaries, and streaming aggregates. Accelerated aggregates validate maintenance state, source generation, metadata and summary versions, field coverage, source counts, and every segment before publishing accelerated metrics. Reasons including `maintenance_pending`, `generation_mismatch`, `metadata_format_mismatch`, `summary_format_mismatch`, `summary_missing`, `summary_checksum_mismatch`, `numeric_summary_requires_rows`, and `typed_summary_requires_rows` select the exact row aggregate.
- Rollup or time-bucket substitution requires a planner proof of equivalence.

Time-series index records, graph adjacency records, and column metadata and summaries are latest-only derived sidecars. Startup audits their version, generation, counts, checksums, and source membership as applicable and rebuilds the complete sidecar when state is missing, malformed, old, or inconsistent. Cassie does not read incompatible derived formats through a compatibility branch; authoritative Midge row records remain the recovery source.

## Storage Layout and Amplification

Query-hot Cassie records use the `cassie-midge-layout-v1` baseline. Hot keys use compact family tags and persistent numeric object identifiers. Names and JSON wrappers are reserved for low-frequency catalog or operational metadata.

Golden fixtures own ordering and round-trip behavior for rows, scalar indexes, full-text postings, vectors, time-series entries, graph adjacency, and column batches. The baseline fixture must show at least a 25% reduction in total query-hot key/value bytes from the fixed pre-change fixture.

Mutation benchmarks report logical mutations, Midge writes, bytes, index maintenance, and derived-state publication. Duplicate replay and no-op updates must not rewrite unchanged hot records. Amplification limits are contract assertions tied to workload shape, not elapsed time.

## Cancellation and Parallel Work

Deadline and cancellation checks occur at batch boundaries and within scoring, joins, aggregation, sorting, graph traversal, provider retries, and operator switching. Internal parallel work acquires permits from one shared per-engine bound so concurrent queries cannot multiply configured worker counts. Ordered merges are deterministic and must not duplicate or omit rows. REST cancellation is acknowledged only after the controlled worker observes cancellation and finishes cleanup; dropping a request alone is not evidence that query work stopped.

REST and pgwire apply write-idle deadlines beneath their protocol implementations. `CASSIE_REST_WRITE_TIMEOUT_MS` and `CASSIE_PGWIRE_WRITE_TIMEOUT_MS` both default to 10,000 milliseconds; a peer that stops accepting output cannot retain a writer indefinitely.

## Benchmark Tier Contract

Cassie follows the `cntryl-stress` Tier 1-6 taxonomy. Tiers 1-4 are the normal developer suite and the sum of their owner wall times must remain at or below 900 seconds. Tier 5 is the manual scaling and saturation suite. Tier 6 is the endurance suite.

| Tier | Measures | Cassie ownership |
| --- | --- | --- |
| 1 - Hot path | One production kernel | Binary `cassie-midge-layout-v1` row and layout codecs, key encoding, predicate and value operations, tokenization and BM25 kernels, vector distances, top-k maintenance, and row serialization. Runtime, storage, async work, the SQL pipeline, synthetic stand-ins, `lexkey-v2`, and JSON row or key wrappers are excluded. Parameter binding and HNSW candidate search belong to Tier 2. |
| 2 - Subsystem | One subsystem operation | Parser, binder, planner, caches, physical operators, posting merge, ANN candidate or probe selection, hybrid fusion, protocol codecs, and one projection write or replay batch over at most 2,048 rows. Full SQL execution, listeners, concurrency, and scale loops are excluded. |
| 3 - System | Embedded end-to-end behavior | Fixed-duration execution of one representative 100k case for each access-path family: relational/index, join, column analytics, full-text, exact/HNSW/IVF vector, hybrid, graph, time-series, lifecycle/startup, and short mixed load. Additional sizes and saturation loops belong to Tier 5. |
| 4 - Integration | A real external boundary | Authenticated loopback pgwire and HTTP servers with real clients, normally sharing a reusable 10k fixture. This tier owns persistent-connection simple and extended queries, portals, cancellation, HTTP operations, and protocol comparison. Client sweeps and sustained connection churn belong to Tier 5. |
| 5 - Scaling/saturation | Curves and limits | Query, retrieval, lifecycle, and transport owners over 10k, 100k, and 250k fixture classes, clients at 1/2/4/8/16, and workers at 1/2/4. Large SQL, join, search, vector, hybrid, replay, rebuild, and concurrency cases belong here. |
| 6 - Soak/endurance | Long-lived stability | Exactly two default scenarios: mixed query/ingest/retrieval over 100k rows, and pgwire/HTTP lifecycle over 10k rows. Each scenario runs for one hour by default and proves correctness, resource bounds, permit accounting, cleanup, and zero failed operations. |

When a scenario changes owners without changing behavior, it keeps its existing scenario ID. When a Tier 3 representative case is intentionally repeated as part of a Tier 5 curve, the scale case uses a distinct `perf.scale.*` ID.

## Typed Runners and Timing

Every benchmark declares a typed `BenchmarkTier`; generic tier constructors are not part of the contract.

| Declared tier | Allowed runner | Timing model |
| --- | --- | --- |
| `BenchmarkTier::Tier1` | `measure_micro` | Production-kernel micro measurement |
| `BenchmarkTier::Tier2` | `measure` or `measure_counted` | One subsystem operation, optionally with an explicit operation count |
| `BenchmarkTier::Tier3` | `measure_batch` | Fixed-duration embedded batches |
| `BenchmarkTier::Tier4` | `measure_batch`; `record_external` only for genuinely external harnesses | Fixed-duration boundary work |
| `BenchmarkTier::Tier5` | `measure_batch` | Fixed-duration scale or saturation batches |
| `BenchmarkTier::Tier6` | `measure_batch`; `record_external` only for genuinely external harnesses | Fixed-duration endurance batches |

The scenario registry declares the tier, operation unit, evidence role, and fixture class for every scenario. Before any measurement, the harness rejects a mismatch between the declared tier and owner prefix, runner or timing mode, fixture class, or fixture size.

External timing records the elapsed interval once. `record_external` receives the completed-operation count and elapsed duration for the whole interval; it never multiplies elapsed time by completed operations.

Correctness, evidence, setup, and configured resource-bound failures are hard gates and panic. Timing noise diagnostics, such as unstable variance or a sample too small for a useful percentile, are reported but remain non-fatal when correctness and evidence are intact.

## Fixtures, Setup, Cache, and SQL

Filtering happens before setup. Fixture construction is lazy, one fixture is reused per owner and scale, and fixture construction plus preflight remain outside measured closures. Preflight may validate fixture counts and plans, but it must not execute or warm the timed statement. Artifacts record setup time separately from measurement time.

Fixture classes are part of scenario ownership: Tier 2 is capped at 2,048 rows; Tier 3 uses one representative 100k case per access-path family; Tier 4 normally reuses 10k rows; Tier 5 owns the 10k/100k/250k curves; and Tier 6 uses the two declared 100k and 10k fixtures. A join fixture must be visible to the actual integration harness before its timed query is eligible to run.

Every network benchmark listener uses a non-empty credential backed by a Cassie role. Passwordless bootstrap is embedded-only and cannot be used to make a listener benchmark pass.

Execution-result caching is disabled for every benchmark owner except the dedicated Tier 2 result-cache benchmark. Every result records the observed cache-hit count so cache isolation is evidence, not a scenario label.

Dynamic SQL values always use bound parameters in benchmarks and their fixtures. SQL formatting is limited to identifiers chosen by a closed, validated helper. Parameterization tests prove that boundary without adding hostile-input examples to benchmark fixtures.

Successful 100k analytical cases use and record the explicit 64 MiB benchmark-only query-memory profile. This replaces proportional column-batch memory overrides and does not change the 10 MiB runtime default.
The preserved dense-join row is the explicit exception: it records `benchmark_resource_profile=dense_stream_selection_4k` and uses a 4 KiB algorithm-selection profile, while the 64 MiB rule applies to column-analytical cases.

Each result records, from observed execution rather than expectation alone:

- environment and configuration labels;
- result cardinality;
- selected access path and fallback reason, with an explicit preflight, operation, or runtime-metrics evidence source rather than a registry declaration;
- storage reads and point fetches;
- candidate counts;
- peak accounted query memory;
- configured worker count and leaked active workers as distinct values; gate rows require the latter to be observed as zero after the sample;
- execution-result-cache hit count;
- setup time and measurement time.

Candidate and fallback metrics are scoped to the scenario's access family. A fallback in an unrelated search, vector, join, analytical, or lifecycle subsystem cannot contaminate another scenario's evidence row. SQL query, system, scaling, and mixed-load gate rows must attach untimed plan preflight evidence before measurement.

Tier 3 column, vector, and graph representatives additionally assert exact fixture results, deterministic ordering, final-path read and candidate bounds, peak accounted memory, and zero live reservations or workers after measurement. Tier 4 portal evidence asserts two ordered, disjoint pages under cumulative limits; cancellation evidence requires `57014`, no cancelled page, bounded reads, and portal/query cleanup. These are correctness gates and do not create latency claims for smoke runs.

## Tier 5 Scale and Saturation

Tier 5 has explicit query, retrieval, lifecycle, and transport owners. Its required curves cover:

- dataset sizes of 10k, 100k, and 250k rows for applicable query, retrieval, replay, rebuild, and lifecycle cases;
- client counts of 1, 2, 4, 8, and 16 for applicable transport and concurrency cases;
- worker counts of 1, 2, and 4 for applicable execution cases.

Every applicable owner emits evidence for every value on its declared axis. These are manual, environment-labelled scale curves, not production capacity claims.

## Tier 6 Duration and Resource Gates

`CASSIE_BENCH_SOAK_DURATION_SECONDS` sets the measured duration for each Tier 6 scenario and defaults to `3600`. The `--soak-duration-seconds` command-line option takes precedence over the environment variable, which takes precedence over the default.

Tier 6 disables warmup and cooldown. It divides the resolved total duration across its measured samples and records both the total and per-sample duration. Smoke validation may explicitly lower the duration through the command-line option or environment variable; that result remains diagnostic rather than endurance evidence.

Durations below 3,600 seconds are rejected outside the smoke profile. The complete-suite validator also requires every Tier 6 owner to declare at least 3,600 configured seconds and every Tier 6 summary to contain at least one hour of observed measured wall time, so shortened artifacts cannot satisfy endurance acceptance.

Both Tier 6 scenarios enforce exact result and state checks, configured memory/cache/result bounds, shared worker permits, connection and task cleanup, and zero failed operations. A complete default Tier 6 run therefore measures at least two hours: one hour for each declared scenario.

## Complete Artifact Manifest

A complete-suite artifact contains one run ID, commit, toolchain, profile, and unfiltered result set for the full declared owner registry. The validator rejects missing or extra owners, mixed run metadata, stale results, filtered artifacts, and fixture or evidence mismatches.

The default Tier 1-4 manifest also sums owner wall time and panics when that total exceeds 900 seconds. Owner wall time includes setup and measurement, while the artifact retains their separate durations for diagnosis.

Filtered and smoke runs are diagnostic artifacts and never overwrite a complete owner suite's `latest.json`. Local fallback artifacts and percentiles are comparative developer evidence only. Production-ready latency, capacity, and disk-backed claims require retained evidence from a named deployment profile.

## Acceptance Commands

Run final benchmark acceptance in this order:

```sh
cargo bench --locked --no-run --bench '*'

STRESS_PROFILE=smoke \
CASSIE_BENCH_SOAK_DURATION_SECONDS=5 \
cargo bench --locked --bench '*'

cargo bench --locked --bench 'tier[1-4]_*'

CASSIE_BENCH_RUN_ID=<unique-run-id> \
cargo bench --locked --bench '*'
```

Run the artifact-manifest integration test against the artifacts produced by the final wildcard run. That final run is intentionally long because it includes both default one-hour Tier 6 scenarios.

## Benchmark Scope Boundary

Benchmark source and test files remain under 1,000 lines. Cassie's suite does not add coverage for Midge durability, WAL, snapshot, or recovery mechanics; those remain Midge responsibilities. Benchmark completion does not by itself close deployment-profile or disk-backed production evidence.
