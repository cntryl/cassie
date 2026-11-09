# JSON object-key flattened row directory boundary

Status: the selected physical SELECT handoff described below is implemented
but remains untested. Other logical, DML, paginated and set consumers still
use bare Vecs and are not qualified by this record. Existing stored JSON
affected by serde's private sentinel interpretation cannot be assumed
recoverable. Cassie continues to use published Midge directly.

## Concrete route and invalidation points

Before this selected handoff, `json_object_keys_chunks::OwnedExpanded::flatten`
allocated a fresh admitted destination, consumed input directories under their
lease, and dropped the destination lease before returning the bare Vec.
`source::finalize_plan_rows` could consume that Vec in set operations and,
for a set plus ORDER BY, sorting. Its pagination fallback passes rows to
`source_rows::slice_rows`, which performs `IntoIter::skip`, optionally `take`,
and `collect`. Neither source nor Rust's public contract guarantees that the
collected Vec uses distinct backing storage. A lease must therefore outlive the
input iterator and any returned allocation that might reuse its backing; an
assumption that the old Vec was destroyed is insufficient. The old reservation
also cannot cover an independently grown destination without separate proof.

The following private consumers of `execute_plan`, `execute_plan_with_outer_row`
and `execute_physical_plan` need distinct owner handoffs before a shared return
signature can be considered covered:

| Consumer | Current operation and owner boundary |
| --- | --- |
| `entrypoints::execute` / `result::build_select_result` | Physical-plan Vec is borrowed while a separately admitted public `QueryResult` is constructed; source rows are explicitly dropped after output finishes. Any error before that point must retain the directory owner through row destruction. |
| CTE execution / `cte::retention::RetainedRows::from_output` | Accepts a bare Vec and separately reserves its capacity after receipt; its iterator converts rows into independently retained entries. A transferred original directory owner must not be double counted, and must outlive the consuming iterator on errors. |
| `subquery_eval` and DML row-subquery | Validate cardinality/width and move entries into separately admitted result directories. Preserve those existing error priorities and the original owner across all partial moves and early returns. |
| `dml_insert` / `insert_source_owner` | Consumes SELECT rows into identity-policy and lazy write inputs; existing parent sidecars protect row payloads, not a detached outer directory. Hold its owner through the consumed Vec iterator independently of payload leases. |
| `dml_joined` | Converts each `BatchRow` to a values Vec and collects a new source directory. The original directory owner follows its iterator; separately audit the new directories and source query/operator parent payload lifetimes. |
| Materialized projection and rollup routes | Rows can be converted into document payloads for synchronous writes. Retain the source directory owner until its iterator is destroyed; existing row payload sidecars and new map/Midge allocations have separate contracts. |
| Views, FROM subqueries, correlated EXISTS and analytical projections | May requalify or rechunk returned rows, inspect `is_empty`, or pass rows into another consumer. Track each intermediate owner until the actual allocation is freed or its ownership is explicitly transferred. |
| Set-operation right branch and modifying CTEs | Recursive executions and their collections can have distinct source controls, directory allocations and parent owners. Preserve input owners during errors and return, and separately admit any new output directory. |
| EXPLAIN timing / `execute_plan_with_execution_breakdown` | Carries rows alongside execution timing or consumes them at an explain boundary. Keep the row owner with the corresponding Vec and preserve timing/error behavior. |
| Optimized/access-path and source early returns | Pull execution, typed/accelerated aggregates, projected reads and other access paths currently also return bare Vecs. A universal wrapper may mark these owners absent only as an explicit legacy fallback; that does not establish their accounting. |

## Proposed private representation

A private `OwnedRows { rows: Vec<BatchRow>, directory: Option<QueryMemoryReservation> }`
would declare rows before the lease, so errors and ordinary destruction drop the
backing first. For the selected checked flatten, transfer the existing destination
lease into this wrapper without an intervening fallible step and verify tracker
identity before any lease absorption. Other legacy producers may initially use
`None` without being described as controlled. Empty output, retained capacity,
zero-byte reservations, different trackers and partial construction need explicit
cases. A row's query/operator Arc alone is not an outer Vec owner: callers can
empty a still-allocated Vec or move its final row elsewhere.

A consuming wrapper needs an `OwnedRowsIntoIter` with the Vec iterator declared
before its lease. Its destructor must drop all remaining rows and the iterator's
backing before the lease on exhaustion, cancellation, error and unwind. It must
not expose a bare iterator or raw Vec that can escape the owner. No mutable Vec
API may grow capacity without prior admission. If an operation can reuse an
input backing allocation, either return that same lease with the resulting Vec
using a proven transformation, or build a distinct separately admitted output
while the input and lease remain live. A fallback to a bare Vec discards the
ownership guarantee and must be explicit. Source and destination payload owners,
shared type parents and directory reservations remain separate.

`slice_rows`, set operations and sorting require individual contracts: preserve
row order, duplicates, LIMIT/OFFSET behavior, comparator semantics and existing
error priority; retain all source owners through a fallible destination build;
and cover source/destination overlap and actual partial-move cancellation.
Identity transformations can keep the original Vec and lease only where exact
behavior is proved. Fresh result arrays/directories require their own admission;
an old reservation does not automatically cover them. Once an operation has
consumed the original allocation, retiring its lease must follow destruction,
not simply the last row move. The same applies to iterator specialization,
empty-but-allocated vectors and partial drain.

The end-to-end design would thread the private wrapper through source execution,
physical and logical dispatch, and every audited consumer above while preserving
the public `QueryResult`, its serialization and client API. Each consuming
adapter must complete its own destination admission before releasing the source
wrapper, or explicitly retain the legacy path and make no coverage claim. Any
extra checked copies, workspace reservations or cancellation points can change
budget and error precedence and must be selected and recorded before runtime
activation. The representation does not solve preexisting source clones,
physical tree/allocator bounds, Arc header retirement, result-vector ownership,
Midge conversion, sentinel semantics, or teardown.

## Decision and proof status

Jev's first scoped gate was inconclusive: local guard 0.37, selected-route design
0.31, full design 0.28; owner/early-return lead 0.79. Receipt:
`target/assembly-review/json-flat-rows-boundary-{request,response}.json`.
This matrix supplies concrete route evidence for the selected scope below; no
model selection establishes correctness. Before broader activation, author tests for
all affected callers, set/sort/slice semantic and order parity, selected and
legacy routes, empty and retained-capacity Vecs, source and destination tracker
identity, exact/below overlapping admission, actual iterator/partial-copy
cancellation and errors, and final result/drop lifetime. Review the exact source
and perform the deferred build and test gate in the prescribed order.

After review of the matrix, Jev selected an isolated test-only owner/iterator
foundation (0.89; propagation lead 0.88), receipt
`target/assembly-review/json-flat-rows-matrix-{request,response}.json`.
That decision alone did not authorize a production return seam.
The isolated probes must cover empty retained capacity, same/foreign tracker,
partial consumption and cancellation/error/unwind, no bare Vec or iterator
escape, and independently admitted source/destination overlap. Consumer-specific
propagation and a complete lifetime proof remain necessary.

The isolated `OwnedRows::new` experiment verifies tracker identity and that the
lease byte count covers the observed Vec capacity. Those checks do not prove
that the reservation belongs exclusively to that Vec or that its allocation was
preadmitted. A production caller must establish both attribution and admission
at the concrete allocation/transfer seam; the test-only constructor does not.

## Selected private physical SELECT route

Jev selected the smallest live route after the caller audit (0.99, ownership
and routing lead 0.80); receipt
`target/assembly-review/json-selected-owned-result-{request,response}.json`.
The selected object-key checked flatten transfers its own exclusive destination
reservation with the Vec. After pagination is resolved, when the plan has no set,
OFFSET or LIMIT, source finalization can preserve that same Vec and lease as an
identity operation. Physical SELECT dispatch carries the wrapper into the
existing result builder, which keeps it through admitted `QueryResult` finish
and drops source rows before the directory reservation. DISTINCT may run first;
its own buffers remain separately unqualified. Empty and nonselected/optimized
paths carry no claimed destination owner.

Logical, CTE, DML, correlated/subquery, set and paginated routes deliberately
return to their existing bare-Vec behavior: stripping the wrapper drops its
lease and does not cover the returned allocation. They are not covered by this
increment. Public `QueryResult` and the direct Midge boundary remain unchanged.
Keeping the source lease through result construction increases overlapping budget
admission and can change resource-error priority for this selected route. Check
same/foreign tracker, identity selection after pagination resolution, actual
object-key SELECT result success and failure, last-owner release, measured
source/result overlap, cancellation, DISTINCT, empty/optimized and unsupported
fallback behavior. All probes are authored before implementation and unexecuted.
