# Private CTE memory retention contract

The selected coherent bundle is #760 plus confirmed accounting defect #857.
Both remain open until their selected acceptance and publication obligations are
complete. This selection preserves existing plain owned rows and independent
context copies. It introduces no public API, persistent format, storage layout,
statement visibility, materialization modifier or recursive capability.

CTE operators retain #760's `scalar_cte_materialization` diagnostic and remain
unpromoted for native execution even after their backing is honestly accounted.

## Selected private ownership

A private controlled CTE context owns the existing name-to-relation map and its
capacity reservation. Removing the last relation releases its row backing but
retains any allocated map capacity until the actual context drops. Context
copies deep-copy rows and fields independently after admitting complete old/new
overlap. They do not share mutable relation contents or change shadowing.

Each materialized relation retains admitted owned row vectors, payloads, names,
field descriptors and nested type backing. Replacement admission occurs before
BatchRow conversion drops the producer's source/operator owners. Aliasing and
recursive growth admit their replacement allocations before construction.
Source references admit independent row copies, descriptors, aliases, lookup
and output mapping before cloning and transfer the replacement charge to the
returned rows. Original context and copied output may drop in either order.

Source construction first admits its complete conservative old/new overlap.
After constructing initial rows, measure their unleased body and eager lookup
backing before attaching an owner. Release expired construction and qualification
allowances while retaining actual row/output capacity, forthcoming chunk overlap,
Arc metadata, descriptors and maximum serializer scratch. String/JSON rows retain
the existing conservative physical-name normalization and text-field inference
allowance; integer-only rows construct no text inference keys. Shared finalization
separately admits replacement qualification aliases and lookup. This refinement
preserves generic join/filter admission and the existing recursive workload budget.

Estimates borrow existing values and use checked capacities and heap estimates.
Serialized JSON length alone is not owned-memory authority, and estimation must
not allocate a serialized copy before admission. Allocation, budget, cancellation
and deadline failures return controlled errors without partial success.

Shared source finalization also admits fresh qualification aliases and eager lookup
for rows with live private source/operator owners. Reuse the existing collection
qualification authority, including actual alias Vec capacity and old/new growth.
Qualify in place, retain the new owner with the existing parent chain, and preserve
ordinary unleased scalar paths. Derived/subquery operator output has explicit
near-budget denial and parent/output-drop controls before this repair is qualified.

## Complete context and recursive paths

Contexts are created at ordinary/physical entrypoints, INSERT SELECT,
materialized-projection execution and view execution. Derived/LATERAL execution
and EXISTS resolution copy contexts. Binder/source-shape field reads borrow
relations while their copied descriptor namespace has a temporary admitted owner; final dispatch and recursive working state insert or replace them.
Existing tests that construct raw relations use explicit controlled fixtures.

Recursive accounting preserves the existing base, UNION/UNION ALL deduplication,
delta and accumulated rows, working-context publication, empty stabilization,
configured depth failure and restoration order. Admit delta copies, seen keys,
working copies and accumulated capacity growth while previous state remains
charged. This is accounting existing recursion, not selecting new recursion or
changing evaluation and visibility.

## Finite acceptance matrix

| ID | Required executed controls |
| --- | --- |
| CTE-01 | Live materialized NULL/BIGINT context remains charged after producer/output drop; context-first/output-first drop both reach zero. |
| CTE-02 | String/vector/escaped JSON and ARRAY descriptors have borrowed pre-admission; tiny-budget failure publishes no replacement. |
| CTE-03 | Multiple source references admit independent old/new overlap, preserve values/aliases/descriptors and deny peak-minus-one copies without dropping the original. |
| CTE-04 | Derived/LATERAL and EXISTS context copies preserve deep-copy shadowing; budget/cancel/deadline cleanup retains only externally live original state. |
| CTE-05 | Existing recursive UNION/UNION ALL, duplicate/empty stabilization and depth failure preserve semantics; growth denial/cancel retains honest state and drops zero. |
| CTE-06 | Empty allocated contexts retain map capacity until drop; all failure paths avoid partial publication and native CTE promotion. |

Write actual failing assertions before the smallest fixes. Preserve source/log
hashes and Jev's specific questions/actions in ignored target evidence. This private repair is integrated into the coherent #760 branch. Complete
ordered validation and exact-head hosted checks and review remain required;
focused qualification is not full-suite, hosted or closure proof.

Map insertion admits borrowed name bytes before cloning the key. Lowercase-name
scratch is admitted before normalization. ARRAY source descriptor replacement
uses borrowed nested-type estimates and retains a distinct owner on returned
owned rows; value and logical descriptor shapes remain unchanged.

The controlled namespace covers its own copied map, field vectors and nested
ARRAY type backing. Known CTE source field copies are admitted before binder
construction, then returned ARRAY descriptor backing has a separate retained
owner. Generic binder inference also builds schemas and inferred output fields;
this change does not establish a new general memory bound for those existing
planner allocations. It must not be described as bounded planner inference.
Actual ARRAY conversion uses explicit admitted type slots rather than reusing
larger FieldSchema vector backing through in-place collection.

Known CTE NULL-row join templates also copy field names, nested ARRAY metadata,
lookup and qualification backing. Their admission must survive until the actual
template drops; later joined-output admission cannot own an earlier template.

CTE-derived Subquery/LATERAL NULL templates use the same retained replacement
owner after existing field inference. Admit the owned inferred fields before
NULL-row/type/lookup/qualification construction and retain them until template
drop. This does not qualify binder inference internal schema/field copies;
direct CTE field-copy admission remains before binder construction.
