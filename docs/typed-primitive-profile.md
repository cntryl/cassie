# Primitive storage-to-typed measurement

This finite measurement owner implements [#1263](https://github.com/cntryl/cassie/issues/1263) from merged baseline `8e3b8ae44346897bfda20ba0f46e43814b04ac0b`, after #1113 closed. Existing typed predicates, numeric aggregate kernels and encoded views remain the source of truth; no reader rewrite or new SQL capability is part of this measurement. The [selected private tracing contract](https://github.com/cntryl/cassie/issues/1263#issuecomment-6880906035) supplements the [typed batch](typed-batch-contract.md), [typed aggregate](typed-aggregate-contract.md) and [columnar authority](columnar-table-contract.md) contracts.

## Finite comparison

All three tables contain the same 100,000 nullable BIGINT/BOOLEAN pairs: heap authority, column authority without an index, and heap authority with a covering derived column index over `(n,b)`. Independent exact integer results and PostgreSQL result types are checked for both predicates.

| Workloads | SQL predicate | Aggregate |
| --- | --- | --- |
| `row_true`, `column_true`, `indexed_true` | `b IS TRUE AND n >= 0` | `SUM(n), COUNT(n), COUNT(*)` |
| `row_not_false`, `column_not_false`, `indexed_not_false` | `b IS NOT FALSE AND n >= 0` | Same exact aggregate |

The independently calculated expected primary result is
`(27021597771773687, 15121, 15121)`; the companion result is
`(54043195543578022, 30288, 30288)`. SUM(BIGINT) has NUMERIC result metadata
(OID 1700), while both COUNT columns use OID 20; result representation and
metadata are checked separately. All six actual SQL diagnostic and timed cases matched those exact values and metadata.

The registered owner is `tier3_system_typed_primitive`, with scenario IDs `perf.typed_primitive.<workload>.100k`. The deterministic data includes NULLs, negative values and integers beyond 2^53; NULL truth sets and integer SUM state remain exact. Query memory is an explicit benchmark-only 2 GiB, native total memory 2 GiB with a 512 MiB memtable, one worker and disabled result cache. These are comparison controls, not minimum-memory or process-RSS claims. Runtime defaults are unchanged.

## Evidence scopes

Actual completed transport/operator observations are captured in a separate untimed SQL pass through a scoped private tracing subscriber, alongside existing acceleration/fallback/read/validation counters. A missing typed event does not establish zero conversions or typed success: dispatch can complete through accelerated aggregation or fallback. Batch events from a failed query describe an attempt; SQL success and exact result verification establish whole-query completion. Lane/carrier counts and admitted query peaks are not allocation counts or process memory.

The timed collector is disabled. Canonical harness samples include query execution and declared result verification, but exclude the untimed observation pass and CPU profiler overhead. CPU sampling uses a separate repeated-query window on the actual executable/PID after fixture setup. Commands, ready/window identities, symbols and sampling limitations belong in the final receipt. An actual allocation census is unavailable unless allocation tooling successfully executes; the presence of an `xctrace` wrapper alone is not that evidence.

## Exact qualification identities

The audit baseline is `8e3b8ae44346897bfda20ba0f46e43814b04ac0b`. Production source was fully qualified at `c3fd17f8e2d9c6893766604e17985bcbef3e1b66`, tree `3fed7b5a3b0379c19f7fe535c23255e72b6a27ab`. The final canonical measurement revision is `e10068c9cf9e9e5533dbcbba8a3c1c17169bd906`, tree `8e602aa77e86fca1f481640a33c2ea74c17db468`; its sole change is six shared catalog worker declarations from unspecified to one. Production, tests, Cargo, benchmark main, fixture, diagnostic and CPU-mode source are byte-identical across those identities. CPU records remain explicitly at the original source and binary SHA; they are not relabeled as the later executable.

Private integration artifacts used jobs=2, test threads=4. All times below are UTC on 2026-11-14.

| Check | Actual result |
| --- | --- |
| `cargo build --locked --bin cassie` | Passed at `c3fd17f8` 17:30:30 |
| `cargo test --locked` | Passed 17:46:16: 13,512 passed, zero failed, nine external-fixture tests ignored; 44 result blocks |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic` | Passed 17:46:56; catalog-only affected repeat passed at `e10068c9` 17:56:46 |
| `cargo fmt --all -- --check` | Passed 17:47:01; affected repeat passed 17:56:51 |
| `cntryl-tools validate-tests -f <path>` | Both touched source-test files passed, including eight new unit probes; affected repeat passed 17:56:51 |
| `tools/test_inventory.py --check` | Generated inventories current; nonzero for unchanged baseline five unregistered pgcrypto tests and one oversized parser file, with no new unresolved modules, duplicate identities or destination conflicts |
| Six-case canonical benchmark and untimed diagnostics | Passed at `e10068c9`, 17:56:57–17:57:39 |
| Separate CPU windows | All three primary cases completed at original `c3fd17f8`, sampler exit zero |
| Final PR-head hosted checks/review/merge | Actual terminal readback is recorded in [#1263](https://github.com/cntryl/cassie/issues/1263) after this receipt is committed |

Eight tiny unit probes in `src/executor/scan/typed/profile_tests.rs` passed: actual document/encoded lanes, identity-versus-payload distinction, exact result equivalence with collection disabled, cancellation, admission rejection, declined grouped dispatch and attempt-scoped events after result failure. The separate benchmark-entry `validate_failure_classification` assertion probe executed before setup/timing in successful benchmark invocations; it is not counted as a Cargo unit test. Source-first probes were authored before instrumentation; no baseline RED was observed. Initial source review corrected identity-clone mislabeling and failed-SQL completion labels. The first integrated build failed E0782 because a tracing macro resolved `Value` to a trait; the fully qualified SQL type repaired it. That failed build is retained separately, with later checks unrun on that attempt.

## Canonical results and observations

Command: `cargo bench --locked --bench tier3_system_typed_primitive -- --output-dir <receipt-directory>/benchmark-artifacts`, with `BENCH_TYPED_PRIMITIVE_MODE=benchmark` and `BENCH_TYPED_PRIMITIVE_EVIDENCE_OUT=<diagnostic-json>`. The cntryl-stress 0.5.1 artifact contains six specs/summaries and 36 samples, default unfiltered complete owner, one warmup and five measured fixed-duration samples per case (500 ms target), acceptable quality and zero failed operations. Both configured and reported catalog worker metadata are one. Timing includes SQL plus the tiny exact result/type check; observation collection and exhaustive oracle construction are outside timing. Disabled trace sites can still incur branch/arithmetic overhead, so no zero-instrumentation-overhead claim is made.

Environment: Apple M5, ten cores, 24 GiB RAM, macOS 27.0.1 arm64, Rust 1.100.0, optimized release build and cntryl-midge 0.5.2. The paired database holds three identical 100k relations, not isolated per-source databases; setup took 6.569 seconds. Cache is off, the query deadline is disabled, scan/aggregate/query worker limits are one, query memory is 2 GiB, and actual native built options are 2 GiB total, 512 MiB memtable/flush threshold and 214,748,364-byte transaction pool. All diagnostic native snapshots were Healthy without write stalls. Runtime defaults are unchanged.

| Workload | Mean queries/s | 95% interval | Operator-boundary admitted peak (bytes) |
| --- | ---: | ---: | ---: |
| `row_true` | 6.2505 | 6.2248–6.2763 | 2,007,137 |
| `column_true` | 3.9858 | 3.8014–4.1703 | 2,007,245 |
| `indexed_true` | 80.5596 | 80.3818–80.7373 | 95,050,760 |
| `row_not_false` | 6.0674 | 5.8465–6.2884 | 2,007,142 |
| `column_not_false` | 4.0594 | 3.9342–4.1845 | 2,007,250 |
| `indexed_not_false` | 77.2372 | 76.7293–77.7450 | 95,050,765 |

These rates describe different existing source layouts under this local profile; they are not a before/after optimization speedup, DuckDB comparison or production-capacity claim. The private peak is the query tracker observed at typed operator completion, not a promise to capture later result-admission peaks. Public runtime high-water counters remain cumulative. Every successful SQL diagnostic ended with zero current accounted query memory; the indexed source owner remains live at the operator event and releases by SQL return.

All six cases observed 98 completed typed transport batches over 100,000 input rows and one typed aggregate completion. Selected input rows were 15,121 for TRUE and 30,288 for NOT FALSE; existing aggregate-acceleration counters remained zero. Preflight EXPLAIN alone named collection scans, so actual batch/counter evidence, rather than that label alone, establishes the encoded indexed handoff.

| Source, same for both predicates | Actual transport | Payload field lanes | Inline Value slots/bytes | Existing read evidence per untimed query |
| --- | --- | ---: | --- | --- |
| Heap, no index | Accounted documents | 200,000 | 200,000 / 9,600,000 | 100,000 source-marker rows; zero column validation |
| Authoritative column, no index | Accounted documents | 200,000 | 200,000 / 9,600,000 | 100,000 source-marker rows plus 400,000 logical validation entries/marker attempts |
| Heap with covering column `(n,b)` | Encoded views: 100,000 numeric and 100,000 JSON-backed Boolean lanes | 0 | 100,000 / 4,800,000, from Boolean conversion | 98 segments, 294 chunks, 394 source-marker observations, 100,000 materialized Boolean values, zero row-blob fetches/fallback scans |

No identity clones occurred for these projected fields; a separate unit proves identity clones are not payload visits. Inline bytes are cumulative slot flow, not live memory or allocation traffic. Native reads, adapter validation attempts and source-marker observations have different units and are not summed into a physical-I/O count. Indexed operator peaks around 95 MB coexist with fewer Value slots because retained source ownership differs; fewer carrier bytes do not prove lower admitted memory.

The original `c3fd17f8` canonical run also completed, but catalog workers were unspecified and the generic harness wrote a zero sentinel over worker metadata. Actual fixture limits/operator events were one. Those historical artifacts remain unchanged; the corrected six-case run above owns canonical reporting.

## Separate CPU sampling and limitations

The actual original executable SHA-256 was `6baccb3e308987d14b5a5a48767da80c354a8d51ddcfcfe27897377f6360f18b` at source `c3fd17f8`. For each primary case, `BENCH_TYPED_PRIMITIVE_MODE=cpu-profile`, the exact `BENCH_TYPED_PRIMITIVE_CASE`, `BENCH_TYPED_PRIMITIVE_PROFILE_SECONDS=30` and a distinct evidence path selected a repeated checked-query window. The ready marker was flushed only after setup and diagnostic preflight, and supplied the actual executable PID. `/usr/bin/sample <PID> 5 1 -file <output>` attached to that PID for five seconds at a requested one-millisecond interval, after ready and before query-window completion. All sampler and process exit codes were zero.

| Primary case | Actual PID | Checked queries in separate window | Actual window seconds |
| --- | ---: | ---: | ---: |
| `row_true` | 40320 | 186 | 30.132 |
| `column_true` | 40428 | 125 | 30.111 |
| `indexed_true` | 40519 | 2,536 | 30.005 |

These loop counts are profiler-overhead observations, not canonical throughput. NOT FALSE companion cases are CPU-unprofiled. The CPU-mode/producer/fixture source is identical after the catalog correction; the old executable hash remains explicitly old, not a claimed hash of the corrected canonical binary. Raw ready/complete markers, command/timestamps, sampler output, addresses and symbols are retained. Rust v0 names are mangled and partly identifiable by embedded function names, not full source-line attribution. Sampling is statistical; nested inclusive branches and unrelated sleeping engine threads must not be summed or treated as exclusive CPU decomposition.

Main-query-thread root samples were 3,456/3,524/3,594 for row/column/indexed respectively. Inclusive `TypedScanStream::next_batch_bounded` branches contained 3,044/3,234/946 samples. The authoritative cursor parents also contain native point lookups, validation and decode work, so these counts do not isolate JSON reconstruction cost. The indexed profile separately observed predicate work, encoded-column construction/JSON-backed conversion and row-ID decoding. Allocator symbols appearing on a CPU stack do not measure allocation counts or bytes. Actual allocation census is unavailable: `/usr/bin/sample` succeeded, while `xcrun --find xctrace` failed under CommandLineTools despite an `/usr/bin/xctrace` wrapper. No tooling was installed and no RSS value is inferred from carrier or admission counters.

## Selected next finite slice

Source-backed review and Jev selected a **separate derived Boolean handoff** lead (choice 0.74, confidence 0.65), rather than a direct authoritative-reader rewrite (0.01). The actual indexed path still converts 100,000 JSON-backed Boolean lanes through 100,000 temporary Value slots while numeric lanes use encoded owners; corresponding construction/JSON branches are observable in CPU samples. This is a bounded, measurable opportunity, not a claim that it dominates global CPU or guarantees a speedup.

The follow-on must preserve the existing format, Boolean NULL/three-valued semantics, exact numeric aggregate state, selected positions, source validation/fallback, captured owner lifetimes, preallocation admission, cancellation and release. Tiny edge probes and the same six-case timing/diagnostic/primary-source CPU comparison must establish its result. No direct reader, engine, format, public API or kernel rewrite is included in this measurement PR. Authoritative-reader cost attribution remains inconclusive until native lookup/validation/decode contributions are separated; existing integrity checks cannot be bypassed to improve a benchmark.

Final exact-source review, hosted-check/review status, one squash merge/tree and #1263 closure readback are recorded in the [owning issue](https://github.com/cntryl/cassie/issues/1263). Raw immutable reviews, all gate attempts, canonical/diagnostic/CPU artifacts and source-equivalence receipts are preserved under ignored `target/assembly-line/typed-primitive-profile/` before task cleanup. This finite local measurement does not establish Linux disk or production capacity.
