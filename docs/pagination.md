# Selected integer pagination

Issue [#797](https://github.com/cntryl/cassie/issues/797) implements the approved
[D2 contract](sql-dialect-profile.md#d2-parameterized-pagination-797) over current
integer types. The broader #792/#793 programs remain open.

`SelectStatement` and `LogicalPlan` carry `Option<Expr>` limit/offset expressions.
Literal bounds use `Expr::IntegerLiteral`; parameters remain zero based;
explicit NULL uses `Expr::Null`; absent bounds and LIMIT ALL use `None`.
The approved public Rust and serialized-query break replaces bare integer JSON
with the existing expression enum serialization. There is no compatibility
wrapper or legacy decoder.

Admitted forms are signed integer literals, NULL, parameters, parentheses,
checked +, - and *, and current SMALLINT/INT/BIGINT casts. A standalone unknown
bound parameter describes as BIGINT, OID 20. Arithmetic parameters need declared
integer types or explicit integer casts; unknown arithmetic rejects with 42725.
An outer result cast does not select inner operand operators. Row references,
functions, division, floating arithmetic, subqueries, aggregates, windows and
volatile expressions remain excluded.

Application execution resolves every query-tree bound under its controls before
cache lookup or physical planning. NULL LIMIT is unbounded; NULL OFFSET is zero.
Negative limits/offsets report 2201W/2201X; checked integer/cast overflow reports
22003. Typed arithmetic retains the selected integer operand domain. Scans,
top-k and materialized portals consume the evaluated literals; bounds never
allocate a vector proportional to their value. A limited portal retains its
original admitted result across suspension and concurrent source changes.

Parameter-dependent physical plans bypass the shape-only plan cache. Result
cache keys still include parameter values, and invalid bounds fail before a
result cache lookup. Direct physical-executor callers resolve preserved logical
expressions before reconstructing their physical selection; the temporary plan
clone reserves query memory before allocation and retains its reservation until
execution finishes. Its streaming size counter rejects more than 512 serialized
containers before a nested plan clone. The preceding read-only traversal returns a controlled resource error at 128
structural levels before counting or cloning, polls cancellation/deadline
controls, and inspects every relevant expression even when a shallow bound
already needs resolution. Bound expressions retain their separate admitted
128-cast envelope. Quoted string containers do not count,
and quoting/escape state persists across writer chunks. Direct bound expressions use the existing 128-level SQL
nesting envelope. Before recursive expression parsing, pagination also rejects
more than 128 arithmetic operators. Existing INSERT, UPDATE and DELETE
expressions resolve nested EXISTS bounds before any mutation; direct EXISTS
execution resolves its bound before applying its one-row shortcut.

## Cache compatibility and authoritative storage

The current plan key already includes `cost_model_version` in
[query.rs](../src/app/query.rs); this slice advances it from 3 to 4. Persisted
plan-entry keys fingerprint that complete key in
[query_cache.rs](../src/runtime/query_cache.rs). Earlier entries therefore miss
under the new key and authoritative SQL is parsed/bound/planned again. The
existing deserialize-rejection path also treats malformed/incompatible cache
entries as a miss and deletes the transient entry.

These entries reside in `StorageFamily::Temp`, the documented temporary/runtime
family in [Database Families](database-families.md). Caches are excluded from
logical database images. This applies the existing versioned cache-rebuild
mechanism; it introduces no catalog, row, field, codec, type/OID or layout
migration, and changes no authoritative stored data. Views retain SQL text,
which is reparsed and its expression bounds are resolved before source access.

## Evidence and remaining gates

The focused SQL reproduction was red on main `4a09c41a`: four tests rejected
parameter/computed bounds at parsing. A sibling DELETE/EXISTS probe separately
failed by deleting rows for LIMIT $1 = 0, and the expression-complexity probe
returned Unsupported instead of a bounded resource failure. Both were fixed. A separate subprocess-contained direct executor probe showed
stack overflow in the pre-admission predicate for a 50,000-level caller-built
CASE/EXISTS tree; the initial traversal now rejects at its finite depth before serialization.

The focused green run passed 11 SQL tests and one wire test. It covers repeated
values, nested/CTE/set and stored-view queries, NULL/ALL, checked integer domains,
hostile magnitudes, serialization/OIDs, direct executor/breakdown calls,
pre-clone memory admission, finite direct bound depth, and DELETE/UPDATE
zero/negative/positive bounds. Wire evidence covers text and binary parameters,
BIGINT descriptors, 2201W/2201X/22003/42725, and cumulative suspended portals.
The independent oracle was PostgreSQL 18.6 (Homebrew aarch64-apple-darwin25.6.0),
with executable SHA256
`db04623906717b3f12df02e2285e8f9990c8ebbb1c9be78a4b1f18afb2361425`.

Jev adversarial probabilities for cache/nested, typing and admission gaps were
0.34, 0.45 and 0.73. Each triggered the focused probes above; Jev is supporting
triage evidence rather than correctness proof. #794 is merged and read back.
The joint build and full locked suite passed 3,415 tests at `4daa803c`; equivalent
subsequent refinements passed final affected controls and all broad quality/
policy gates at `c8d06d8a`, with explicit full-suite provenance retained in the
[joint closeout](alias-pagination-plan.md#local-validation-closeout-2026-10-07).
Publication-base equivalence, exact-head hosted checks, squash merge and
source/issue readback remain pending. This document claims local readiness,
not merged support or production qualification.
