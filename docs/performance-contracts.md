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

SQL parsing rejects text over 1 MiB, more than 100,000 lexical tokens, nesting deeper than 128, nested block comments deeper than 128, and pgwire simple-query batches over 256 statements. These failures are HTTP `400` or SQLSTATE `54000`. REST request bodies are collected through an 8 MiB limiter. Once a body crosses the bound, Cassie discards subsequent frames through end-of-stream before returning HTTP `413`; excess bytes are never retained, and a well-formed HTTP/1 connection remains reusable. Body idle and complete-request deadlines continue to bound that drain. Pgwire emits row descriptions, rows, and command completion incrementally, caps each backend frame at 16 MiB, and retains the generic 16 MiB frontend-frame cap for non-SQL bind and COPY data. Admin UI files retain their 8 MiB cap and stream in chunks no larger than 64 KiB.

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
- Column-batch execution uses typed vectors, validity and selection vectors, segment summaries, and streaming aggregates. Its canonical derived layout is a little-endian `CBM2` manifest plus separate `CBR2` row-ID and `CBC2` field chunks. Manifest publication is atomic, segment IDs are immutable, revisions fence chunk generations, and row ranges are stable and half-open. SHA-256 protects every required chunk. Codec tag 5 identifies ALP and tag 6 identifies FSST; both manifest codec versions are 1. Unknown codec tags or versions fail closed instead of entering a compatibility reader. A missing, corrupt, stale, unsupported, or over-limit required chunk aborts the complete accelerated attempt and selects authoritative rows; partial accelerated results are never returned.
- Column scans follow summary pruning, predicate-chunk reads, a selection bitmap, requested projection-chunk reads, and selected-value materialization. Dictionary projection chunks validate their complete framing, dictionary, and index stream while allocating scalar values only for selected positions. Supported encoded predicates are conjunctions of equality, range, `IS NULL`, and `IS NOT NULL`, with executor rechecking for exact SQL semantics. Unrequested chunks receive no reads or decodes. Transaction overlays, grouped aggregates, joins, unsupported expressions, and semantics without an exact proof use the row path.
- Codec selection is deterministic and automatic. Plain fixed-width or bounded variable-width encoding is the baseline. Boolean and null bitmaps, constant values, typed RLE, sorted dictionaries with bit-packed indices, checked 128-value frame-of-reference blocks, FSST UTF-8 symbol streams, and ALP decimal-scaled float blocks are eligible. FSST uses at most 256 deterministic symbols and a bounded 64 KiB UTF-8 symbol table. Its selective decoder validates the complete table, value framing, symbol indices, decoded sizes, and UTF-8 stream while allocating strings only for selected positions. ALP considers finite values at deterministic decimal scales, requires exact IEEE-754 bit reconstruction, and falls back to plain for non-finite values, signed zero, overflow, or non-exact representations. A codec must save at least `max(32 bytes, 5% of the complete plain representation)` and ties prefer lower decode complexity. Temporal values remain string-backed and complex values remain bounded plain. Cassie does not layer LZ4 or Zstd over Midge compression.
- Accelerated aggregates validate maintenance state, source generation, manifest and summary versions, field coverage, source counts, and every required segment before publishing success metrics. Summary-only aggregation remains valid for eligible unfiltered queries. Filtered ungrouped `COUNT`, `SUM`, `AVG`, `MIN`, and `MAX` may execute over encoded numeric streams and a selection bitmap only when exact row-path semantics are proven. Reasons including `maintenance_pending`, `generation_mismatch`, `metadata_format_mismatch`, `summary_format_mismatch`, `summary_missing`, `summary_checksum_mismatch`, `numeric_summary_requires_rows`, and `typed_summary_requires_rows` select the exact row aggregate.
- Rollup or time-bucket substitution requires a planner proof of equivalence.

Time-series index records, graph adjacency records, and column metadata and summaries are latest-only derived sidecars. Startup audits their version, generation, counts, checksums, and source membership as applicable and rebuilds the complete sidecar when state is missing, malformed, old, or inconsistent. Cassie does not read incompatible derived formats through a compatibility branch; authoritative Midge row records remain the recovery source.

## Storage Layout and Amplification

Query-hot Cassie records use the `cassie-midge-layout-v1` baseline. Hot keys use compact family tags and persistent numeric object identifiers. Names and JSON wrappers are reserved for low-frequency catalog or operational metadata.

Golden fixtures own ordering and round-trip behavior for rows, scalar indexes, full-text postings, vectors, time-series entries, graph adjacency, and column batches. The baseline fixture must show at least a 25% reduction in total query-hot key/value bytes from the fixed pre-change fixture.

Mutation benchmarks report logical mutations, Midge writes, bytes, index maintenance, and derived-state publication. Duplicate replay and no-op updates must not rewrite unchanged hot records. Amplification limits are contract assertions tied to workload shape, not elapsed time.

Column-batch mutations target stable half-open row-ID ranges. A segment targets
`segment_size`, may grow to `2 * segment_size`, and splits at the median when it overflows;
the left half retains its segment ID and the right half receives a new ID. One-row DML
rewrites one segment, or two only when it causes a split. Empty segments extend an adjacent
range and are removed without normal-path merging. Publication, debt fencing, restart
rebuild, and orphan cleanup must preserve exact row fallback under every failure.

## Cancellation and Parallel Work

Deadline and cancellation checks occur at batch boundaries and within scoring, joins, aggregation, sorting, graph traversal, provider retries, and operator switching. Internal parallel work acquires permits from one shared per-engine bound so concurrent queries cannot multiply configured worker counts. Ordered merges are deterministic and must not duplicate or omit rows. REST cancellation is acknowledged only after the controlled worker observes cancellation and finishes cleanup; dropping a request alone is not evidence that query work stopped.

REST and pgwire apply write-idle deadlines beneath their protocol implementations. `CASSIE_REST_WRITE_TIMEOUT_MS` and `CASSIE_PGWIRE_WRITE_TIMEOUT_MS` both default to 10,000 milliseconds; a peer that stops accepting output cannot retain a writer indefinitely.

## Benchmark Tier Contract

Cassie follows the `cntryl-stress` Tier 1-6 taxonomy. Tiers 1-4 are the normal developer suite and the sum of their owner wall times must remain at or below 900 seconds. Tier 5 is the manual scaling and saturation suite. Tier 6 is the endurance suite.

| Tier | Measures | Cassie ownership |
| --- | --- | --- |
| 1 - Hot path | One production kernel | Binary `cassie-midge-layout-v1` row and layout codecs, canonical column scalar codec encode/decode, key encoding, predicate and value operations, tokenization and BM25 kernels, vector distances, top-k maintenance, and row serialization. Runtime, storage, async work, the SQL pipeline, synthetic stand-ins, `lexkey-v2`, and JSON row or key wrappers are excluded. Parameter binding and HNSW candidate search belong to Tier 2. |
| 2 - Subsystem | One subsystem operation | Parser, binder, planner, caches, physical operators, selective encoded column scans, posting merge, ANN candidate or probe selection, hybrid fusion, protocol codecs, and one projection write or replay batch over at most 2,048 rows. Full listeners, concurrency, and scale loops are excluded. |
| 3 - System | Embedded end-to-end behavior | Fixed-duration execution of one representative 100k case for each access-path family: relational/index, join, column analytics, full-text, exact/HNSW/IVF vector, hybrid, graph, time-series, lifecycle/startup, and short mixed load. Additional sizes and saturation loops belong to Tier 5. |
| 4 - Integration | A real external boundary | Authenticated loopback pgwire and HTTP servers with real clients, normally sharing a reusable 10k fixture. This tier owns persistent-connection simple and extended queries, portals, cancellation, HTTP operations, and protocol comparison. Client sweeps and sustained connection churn belong to Tier 5. |
| 5 - Scaling/saturation | Curves and limits | Query, retrieval, lifecycle, and transport owners over 10k, 100k, and 250k fixture classes, including column decode curves and one-row column DML amplification; clients at 1/2/4/8/16; and workers at 1/2/4. Large SQL, join, search, vector, hybrid, replay, rebuild, and concurrency cases belong here. |
| 6 - Soak/endurance | Long-lived stability | Exactly two default scenarios: mixed query/ingest/retrieval over a 100k-row indexed query fixture with a dedicated transient mutation collection, and pgwire/HTTP lifecycle over 10k rows. Each scenario runs for one hour by default and proves correctness, resource bounds, permit accounting, cleanup, and zero failed operations. |

Tier 2 projection-replay samples use one shared Cassie runtime but consume separate, prebuilt empty projection lanes. Each timed batch therefore exercises the same 2,048-event production replay path from an equivalent source position without including fixture setup or accumulating prior samples' event history. Runtime evidence remains scoped to the shared runtime, and the gate continues to report one logical event per applied event.

When a scenario changes owners without changing behavior, it keeps its existing scenario ID. When a Tier 3 representative case is intentionally repeated as part of a Tier 5 curve, the scale case uses a distinct `perf.scale.*` ID.

## Typed Runners and Timing

Every benchmark declares a typed `BenchmarkTier`; generic tier constructors are not part of the contract.

| Declared tier | Allowed runner | Timing model |
| --- | --- | --- |
| `BenchmarkTier::Tier1` | `measure_micro` | Production-kernel micro measurement |
| `BenchmarkTier::Tier2` | `measure`, `measure_counted`, or `measure_counted_batch` | One subsystem operation, optionally repeated in a bounded batch with explicit normalized operation counts |
| `BenchmarkTier::Tier3` | `measure_batch` | Fixed-duration embedded batches |
| `BenchmarkTier::Tier4` | `measure_batch`; `record_external` only for genuinely external harnesses | Fixed-duration boundary work |
| `BenchmarkTier::Tier5` | `measure_batch` | Fixed-duration scale or saturation batches |
| `BenchmarkTier::Tier6` | `measure_batch`; `record_external` only for genuinely external harnesses | Fixed-duration endurance batches |

The scenario registry declares the tier, operation unit, evidence role, and fixture class for every scenario. Before any measurement, the harness rejects a mismatch between the declared tier and owner prefix, runner or timing mode, fixture class, or fixture size.

External timing records the elapsed interval once. `record_external` receives the completed-operation count and elapsed duration for the whole interval; it never multiplies elapsed time by completed operations.

The two 128-item binder and planning owners batch 256 fixture invocations per measured sample and
record `fixture_invocations_per_sample=256`. The elapsed interval covers the full batch, while the
completed count remains normalized to statements, parameters, or plans. This raises sub-millisecond
rows above timer noise without changing scenario IDs, fixture identity, or logical operation units.
The 2,048-candidate posting-merge owner batches 512 complete production merges per measured sample
and retains `candidate` normalization, lifting the fixed-operation window above scheduler noise.
The 2,048-candidate hybrid-fusion owner applies the same 256-invocation bounded window and records
the exact aggregate result-cardinality and candidate counts, while retaining `candidate` as its
logical operation unit.

Correctness, evidence, setup, and configured resource-bound failures are hard gates and panic. The default and smoke profiles report timing-noise diagnostics without making them fatal when correctness and evidence are intact. The release profile additionally requires every intended optimization gate to retain gate-quality trust, so unstable variance, sub-resolution timing, or an invalid measurement shape fails that owner instead of producing canonical evidence.

`document_create_get/10k` is retained as diagnostic mutation evidence rather than an intended release regression gate. Creating documents against the fully indexed 10k fixture triggers non-stationary index maintenance, so shared-runner timing variance is evidence about the mutation environment, not a stable transport regression signal. The Tier 4 HTTP query and vector-search rows remain the trustworthy release gates; the query uses a fifteen-second measured window to average transport scheduling without changing its per-request logical unit. The document row still enforces request completion, response correctness, runtime evidence, and artifact retention. Tier 4 and Tier 5 time exactly the named create and get requests, while the Tier 6 soak uses an explicit create/get/delete cycle to keep its hour-long fixture bounded.

## Fixtures, Setup, Cache, and SQL

Filtering happens before setup. Fixture construction is lazy, one fixture is reused per owner and scale, and fixture construction plus preflight remain outside measured closures. Preflight may validate fixture counts and plans, but it must not execute or warm the timed statement. Artifacts record setup time separately from measurement time.

Runtime evidence records an explicit `storage_read_unit`. Bucket-native time-series queries report logical `time_series_bucket` reads from the access-path counter. Other embedded paths report `runtime_storage_read`; storage reads, candidate counts, peak query memory, result-cache hits, and fallback counts are per-sample scalar observations because cache and duration-batch completion can legitimately vary. The complete-manifest validator uses each observation's worst-case maximum and continues to accept legacy invariant metadata. Setup must not warm a timed query to hide cold-versus-cached behavior.

Duration-based batches record their actual completed logical-operation count and normalize cumulative storage-read, candidate, result-cache, and fallback counters to one operation. The Tier 3 time-series representative executes two unchanged queries per measured closure, declares `query` as its logical unit, and reports the exact 512-row cardinality of one query rather than the summed batch cardinality. This batching changes only measurement shape; its SQL, fixture, setup boundary, path and fallback proofs, sampling, thresholds, and trust policy remain unchanged.

Full-index representative and scale fixtures load source documents in bounded batches of at most 5,000 rows before creating full-text and scalar indexes. This produces the final full-text artifact once after source loading instead of repeatedly rebuilding the growing whole-index state inside each untimed document batch.

Initial vector-index publication and vector-index removal page normalized vectors, HNSW nodes, and
IVFFlat memberships through data transactions of at most 5,000 sidecars. The vector manifest is
published after initial batches and removed after cleanup batches, so a failed attempt remains
retryable without exposing partial indexed state.

Tier 3 join, graph, and time-series domain fixtures use the same 5,000-row transaction ceiling. Fresh graph-edge batches accumulate the generation-bound adjacency manifest across batches, while time-series batches incrementally maintain the pre-created bucket index. Rollup and retention structures belong to their Tier 5 lifecycle fixture and are not prepared by the Tier 3 window-scan owner. The representative 100k scale, query semantics, and Midge response timeout remain unchanged.

Fixture classes are part of scenario ownership: Tier 2 is capped at 2,048 rows; Tier 3 uses one representative 100k case per access-path family; Tier 4 normally reuses 10k rows; Tier 5 owns the 10k/100k/250k curves; and Tier 6 uses the two declared 100k and 10k fixtures. A join fixture must be visible to the actual integration harness before its timed query is eligible to run.

Every network benchmark listener uses a non-empty credential backed by a Cassie role. Passwordless bootstrap is embedded-only and cannot be used to make a listener benchmark pass.

Execution-result caching is disabled for every benchmark owner except the dedicated Tier 2 result-cache benchmark. Every result records the observed cache-hit count so cache isolation is evidence, not a scenario label.

Dynamic SQL values always use bound parameters in benchmarks and their fixtures. SQL formatting is limited to identifiers chosen by a closed, validated helper. Parameterization tests prove that boundary without adding hostile-input examples to benchmark fixtures.

Successful 100k analytical cases use and record the explicit 64 MiB benchmark-only query-memory profile. This replaces proportional column-batch memory overrides and does not change the 10 MiB runtime default.
The Tier 5 250k analytical curve records a separate 96 MiB benchmark-only profile; it remains a
hard bound and uses a 120-second per-query timeout so the curve measures completion rather than the
30-second runtime default. Neither setting widens runtime defaults or the 100k representative
contract.
The preserved dense-join row is the explicit exception: it records `benchmark_resource_profile=dense_stream_selection_4k` and uses a 4 KiB algorithm-selection profile, while the 64 MiB rule applies to column-analytical cases.

The adaptive column acceptance gates are:

- no selected codec exceeds the complete plain encoded size;
- unrequested chunks receive zero reads and decodes;
- incompressible fixtures have no more than 5% p95 regression from the plain path;
- the representative wide-text compression workload retains its selected codec only when
  end-to-end p95 improves by at least 15%;
- representative ALP chunks reduce complete plain bytes by at least 75% while the paired
  selective query remains within 5% p95 of forced plain;
- selected compressible fixtures reduce derived-format bytes by at least 25%;
- selective results, fallback, freshness, cancellation, and memory limits remain exact
  under corruption and publication failures.

Tier 2 owns paired, same-fixture acceptance rows for those latency gates:
`perf.column.selective_encoded_scan.2k` is compared with its forced-plain baseline at a
maximum p95 ratio of `0.85` and batches 256 exact queries per measured sample, while
`perf.column.incompressible_adaptive_scan.2k` is compared with its forced-plain baseline at a
maximum ratio of `1.05` and batches 1,024 exact queries. The benchmark validates codec choices
before timing. Forced-plain
rebuild is benchmark-only and is not a SQL or runtime configuration surface.

The ALP-specific pair uses the same distributed 2k float fixture and batches 1,024 exact queries per
measured sample. `perf.column.alp_selective_scan.2k` is compared with its forced-plain baseline at a maximum p95 ratio of `1.05`; every candidate chunk must also use no more than 25% of its plain decoded bytes. Exact scale predicates are evaluated over checked scaled integers and only selected floats are reconstructed. Non-exact literals use the general semantic comparison path.

The FSST-specific pair uses a 2k high-repetition UTF-8 fixture, places one matching row in each
256-row segment, and batches 512 exact queries per measured sample. Every candidate chunk must
select FSST and use no more than 75% of its plain decoded bytes. `perf.column.fsst_selective_scan.2k` is compared with its forced-plain baseline at a maximum p95 ratio of `1.05`. Candidate and baseline execute in alternating order within the same sampling invocation so host drift cannot manufacture an advantage. The predicate scans every segment while selected projection validates every encoded value and materializes only the eight matching strings.

All four Tier 2 column pairs compile their physical plans before measurement and time only
session-aware physical execution over the active Midge deployment profile. Parser, binder,
planner, cache orchestration, and durable operator-feedback persistence are excluded from this
subsystem signal. Each candidate/baseline pair owns an independent runner group and alternates
one-query `ABBA` and `BAAB` blocks within and across samples; exact row counts and column-path
counters remain correctness gates.

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
- column encoded-query and one-row DML write-amplification curves at 10k, 100k, and 250k rows;
- client counts of 1, 2, 4, 8, and 16 for applicable transport and concurrency cases;
- worker counts of 1, 2, and 4 for applicable execution cases.

Every applicable owner emits evidence for every value on its declared axis. These are manual, environment-labelled scale curves, not production capacity claims.

## Tier 6 Duration and Resource Gates

`CASSIE_BENCH_SOAK_DURATION_SECONDS` sets the measured duration for each Tier 6 scenario and defaults to `3600`. The `--soak-duration-seconds` command-line option takes precedence over the environment variable, which takes precedence over the default.

The canonical local evidence run declares
`CASSIE_BENCH_DEPLOYMENT_PROFILE_ID=workstation-apple-m5-arm64-apfs`. The runner
rejects unknown profile IDs and records the selected profile in every artifact. This arm64/APFS
profile is valid Cassie evidence, but it does not replace representative native-Linux evidence.
The profile definitions, artifact identity, retention, and ownership contract are maintained in
[Deployment Profiles](deployment-profiles.md).

Tier 6 disables warmup and cooldown. It divides the resolved total duration across its measured samples and records both the total and per-sample duration. Smoke validation may explicitly lower the duration through the command-line option or environment variable; that result remains diagnostic rather than endurance evidence.

Durations below 3,600 seconds are rejected outside the smoke profile. The complete-suite validator also requires every Tier 6 owner to declare at least 3,600 configured seconds and every Tier 6 summary to contain at least one hour of observed measured wall time, so shortened artifacts cannot satisfy endurance acceptance.

Both Tier 6 scenarios enforce exact result and state checks, configured memory/cache/result bounds, shared worker permits, connection and task cleanup, and zero failed operations. A complete default Tier 6 run therefore measures at least two hours: one hour for each declared scenario.

Transport-soak throughput is retained as diagnostic evidence because its sustained create/get/delete lifecycle intentionally changes the disk-backed store throughout the run. Completed-operation variance across equal wall-clock windows therefore describes non-stationary transport churn rather than a stable optimization baseline. Its result, resource, cleanup, and duration requirements remain hard gates, and the artifact retains every sample so degradation remains visible.

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

The scheduled [Bench workflow](../.github/workflows/bench.yml) exercises Tiers 1-4 and retains its
stress artifacts using the shared Fitz workflow topology. A manual dispatch executes seven
independent unfiltered shards for Tiers 1-5 and the two Tier 6 soak owners. Shards use one run ID,
commit, toolchain channel, profile, and evidence contract; `fail-fast: false` lets every independent
shard report its result when another shard fails, and each retains the established six-hour timeout
ceiling so parallelism does not narrow the evidence envelope. A downstream job downloads the successful shard
artifacts into one `target/stress` tree, applies the unchanged complete-suite validator, and retains
the canonical `latest.json` owner artifacts only when the entire manifest passes. Dispatch it only
on the commit being evidenced, with a unique run ID and a deployment profile matching the runner;
a smoke duration remains diagnostic and cannot pass the complete-suite validator.

## Benchmark Scope Boundary

Benchmark source and test files remain under 1,000 lines. Cassie's suite does not add coverage for Midge durability, WAL, snapshot, or recovery mechanics; those remain Midge responsibilities. Benchmark completion does not by itself close deployment-profile or disk-backed production evidence.
