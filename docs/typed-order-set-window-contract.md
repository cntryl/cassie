# Private typed ordering, set and window contract

The integration owner selected this finite private implementation matrix for
[#760](https://github.com/cntryl/cassie/issues/760) after #758 and #759 merged in
`06195f0fc9c1e63a5236903976c07f480176dd8a` and were read back CLOSED.
The coherent bundle is #760 only; #761 owns later relational qualification.
This selection preserves the [typed batch contract](typed-batch-contract.md),
[type contract](type-contract.md) and [performance contract](performance-contracts.md).
It introduces no public API, persistent bytes, type identity, spilling or new
cross-phase visibility contract. Selection is not executed acceptance.

## Selected capability and phase matrix

| Operator | Native typed selection | Preserved boundary |
|---|---|---|
| DISTINCT / DISTINCT ON | Tuple keys of NULL, SMALLINT, INT, BIGINT, FLOAT and BOOLEAN; direct keys for DISTINCT ON | Rich keys and expressions retain admitted shared semantic adaptation. Ordinary DISTINCT retains first input occurrence. DISTINCT ON follows its complete order before projection. |
| UNION / UNION ALL / INTERSECT / EXCEPT | Same primitive tuples with existing branch widths and exported names/types | Shared semantic equality; existing UNION ALL signature order and multiplicity, other sets' signature order/deduplication. Existing absent ALL syntax remains absent; right source is uncapped input. |
| Full sort / top-k | Direct primitive columns and safely resolved passthrough aliases, ASC/DESC and explicit/default NULL order | Direct rich keys use borrowed-value pre-admission and the existing typed semantic authority. Expressions retain the named scalar boundary below. Preserve exact semantic numeric comparison, row identity ties and full-sort prefix pagination. |
| Windows | ROW_NUMBER, RANK, DENSE_RANK; existing one-argument LAG/LEAD at offset1; FIRST_VALUE/LAST_VALUE direct typed payload | Primitive partition/order keys, rich key adapters. FIRST/LAST selects existing ROWS bounds with EXCLUDE NO OTHERS, default peer RANGE or whole unordered partition. Explicit RANGE offsets, GROUPS and other exclusions preserve scalar fallback. |

CTE materialization bodies retain the named `scalar_cte_materialization` boundary
for these new #760 operators. The existing private CTE plain-row/context clone
model and its SQL values/types/visibility remain unchanged. Outer operators over
CTE source copies also use this boundary until retained owner provenance and
pre-conversion replacement admission are independently established. This selection
does not promote existing CTE materialization accounting to a bounded native claim.
A focused body-native-decline/differential and budget/cleanup probe is required;
confirmed baseline accounting defects belong to an evidence-linked follow-up.
The existing plain-row CTE owner gap is recorded in [#857](https://github.com/cntryl/cassie/issues/857)
with the bound live-row/accounted-zero probe; #760 does not silently fix or promote it.

ORDER BY expressions and user functions retain `scalar_expression_order` when
they are not direct columns or safely resolved passthrough aliases. Existing SQL
values, errors, direction/NULL policy and deterministic ties remain supported by
the existing controlled scalar implementation. Its result allocation precedes
key conversion admission; this boundary therefore makes no native or bounded
admission claim. The finite selected path does not expand expression estimators.
Direct rich keys qualify only when their borrowed value, type adaptation and tie
allocation can be admitted before cloning/conversion. CTE and expression
boundaries are explicit unpromoted dispositions for VEX-14 and VEX-22; selected
primitive/direct-rich order paths must satisfy both invariants. Empty selected
relational inputs check query controls and return without operator allocation.

Primitive keys use declared metadata when it agrees with their physical carriers.
Rows without metadata qualify only after every cell has been inspected: homogeneous
non-NULL integer, FLOAT or BOOLEAN cells select their private physical carrier;
all-NULL keys select private Null. Within-column integer/FLOAT mixtures and
NULL-first rich keys decline before native conversion. Independently typed set
branches retain exact shared numeric equality. This physical inference never
changes an output row's logical descriptor or scalar carrier.

All payload families and descriptor/carrier provenance survive views/reordering.
NaN/signed-zero/mixed exact integers reuse existing SemanticValue policy; no new
PostgreSQL NaN parity. ARRAY comparison remains its shared typed-element authority.
Window peer equality must not include deterministic row identity tie breakers.
Grouping/HAVING → windows → sort/DISTINCT ON → projection/DISTINCT → set
finalization/set-order → OFFSET/LIMIT remains the current phase order.
Eligibility decline precedes native input conversion; admitted failures are
terminal, never retried on partially consumed scalar input. Existing ordered
index/pull LIMIT/EXISTS and exact-vector streaming paths retain their authority.

## Blocking estimates, admission and ownership

Full sort and windows initially block with admitted O(n) backing and state.
DISTINCT/set retains admitted global key/membership state and selected output
mapping. Top-k uses an O(k) key heap, where k includes offset, but may retain
O(n) source parent backing through gather views. It is a blocking relational
operator: no O(k) total-memory, encoded fusion or streaming claim is made.
Exact vector top-k retains its separate bounded-heap contract unchanged.

Before construction/growth admit actual capacities, tuple/key/identity heaps,
indices/maps, peer/partition state, descriptors and old-plus-new overlap.
Existing immutable source backing is charged once by its owner; copied backing
requires new admission. Budget/overflow failures are controlled errors, without
partial native output or success diagnostics. Retain every parent lease until
its last output view drops. Huge-parent/small-winner probes must verify honest
parent retention, budget denial and release, rather than assume compaction.
Sequential FLOAT key normalization/comparison admits64bytes of formatter scratch;
the shared finite ±2^63 guard limits its integral formatting to20characters.
This is a conservative sequential scratch bound, not a parallel or arbitrary
FLOAT decimal-format bound. Cancellation/deadline checks occur during keys,
partition work and handoff.

## Finite invariant acceptance plan

| Owner | Selected obligation / required focused evidence |
|---|---|
| EXEC-04 | Existing phase order, grouped/HAVING fallback, peers/aliases/DISTINCT/OFFSET integrated scalar comparison. |
| EXEC-05 | Typed sort/top-k equals scalar complete order/prefix for NULL, numeric, rich fallback, ties and k/offset. |
| EXEC-06 | Exact shared equality across numeric equivalence classes, signed zero, NULL duplicates and sets. |
| VEX-13 | Native primitive tuple DISTINCT/set and admitted rich adapters; all split boundaries, duplicate first occurrence, branch width/name and multiplicity. Native GROUP BY remains #758's selected boundary. |
| VEX-14 | Primitive native ordering, rich ARRAY/temporal semantic adapter, exact big integers, deterministic identity ties and finite NaN policy. |
| VEX-15 | Selected ranking/offset/value frame laws across partitions/peers/chunks; empty frames, NULL/out-of-range, nonselected frame fallback and payload/descriptor preservation. |
| VEX-22 | Documented accounted blocking state; pre-admission failure, cancellation, parent/eviction/output drop and zero final reservations. |

Every row remains pending until its exact implementation and focused evidence
complete. Existing scalar tests are oracles, not proof of native execution.
Implement DISTINCT/set, then sort/top-k, then windows as separate red/green
increments. Once coherent, run build → complete locked suite → full workspace/
all-target/all-feature pedantic Clippy → fmt → each touched test validator,
then repository documentation/benchmark/module-size policies. Exact-head hosted
review/checks, squash and source/issue readback remain coordinator work.
