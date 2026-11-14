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
metadata are checked separately. These are arithmetic/source oracles until
actual SQL execution is recorded below.

The registered owner is `tier3_system_typed_primitive`, with scenario IDs `perf.typed_primitive.<workload>.100k`. The deterministic data includes NULLs, negative values and integers beyond 2^53; NULL truth sets and integer SUM state remain exact. Query memory is an explicit benchmark-only 2 GiB, native total memory 2 GiB with a 512 MiB memtable, one worker and disabled result cache. These are comparison controls, not minimum-memory or process-RSS claims. Runtime defaults are unchanged.

## Evidence scopes

Actual completed transport/operator observations are captured in a separate untimed SQL pass through a scoped private tracing subscriber, alongside existing acceleration/fallback/read/validation counters. A missing typed event does not establish zero conversions or typed success: dispatch can complete through accelerated aggregation or fallback. Batch events from a failed query describe an attempt; SQL success and exact result verification establish whole-query completion. Lane/carrier counts and admitted query peaks are not allocation counts or process memory.

The timed collector is disabled. Canonical harness samples include query execution and declared result verification, but exclude the untimed observation pass and CPU profiler overhead. CPU sampling uses a separate repeated-query window on the actual executable/PID after fixture setup. Commands, ready/window identities, symbols and sampling limitations belong in the final receipt. An actual allocation census is unavailable unless allocation tooling successfully executes; the presence of an `xctrace` wrapper alone is not that evidence.

## Qualification and next-slice decision

The coherent source bundle has not been frozen or executed yet. Source-first instrumentation probes are authored/not run, not observed RED. The assembler will record exact source/tree identities, full ordered local checks, all six canonical cases, diagnostic traces, profiler/tool availability, final-head hosted checks and merged-tree readback before closeout. The measured next-slice decision may be inconclusive; latency alone cannot establish a CPU or allocation bottleneck.
