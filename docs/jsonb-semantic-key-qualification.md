# JSONB relational key qualification

Issue #809 selects exact JSONB object/value identity and ordering under the
[Type Contract](type-contract.md#selected-jsonb-contract-2026-10-24). The source
candidate described here is **awaiting the combined executable gate**. Authored
regressions, PostgreSQL oracle results and Jev source reviews are not Cassie
passing runtime evidence. Support remains Experimental.

## Selected behavior

Only a declared JSONB value, or a declared JSONB array element, selects the new
key. SQL TEXT and the established general JSON carrier keep their existing
identities. SQL NULL remains a SQL null key; JSON document null is a non-NULL
JSONB value. Invalid declared documents return typed errors.

JSONB keys compare exact normalized decimal values, including equivalent scale
and exponent spellings and signed zero. The root empty array precedes root
scalars. Nested values follow null, string, number, boolean, array, object order;
array length precedes elements, and object field count precedes canonical
length/byte-ordered field traversal. Field names themselves compare by bytes.
SQL JSONB arrays compare ordered elements before dimensions and lower bounds,
with SQL NULL array slots distinct from document-null slots.

## Private implementation boundary

`types/semantic/jsonb.rs` builds a flat immutable token key behind an Arc.
Numeric tokens retain the existing normalized `NumericKey` identity alongside
sparse decimal digits and weight. Construction validates existing numeric format
bounds and checks cancellation between coefficient chunks; it never expands a
short exponent token into a dense power of ten. Comparisons and hashes allocate
no workspace. Canonical object sorting checks interruption too.

Parse, coefficient, directory, string, and Arc backing are admitted before
allocation. Each document key and SQL-array directory retains its own narrow
memory reservation through the last key alias. Neither retains request deadline
or cancellation state. Error paths release partial keys. This changes no public
Value representation, persisted index encoding, row bytes, catalog layout,
storage engine, dependency version, or physical join selection.

Private declared-type metadata reaches scan, VALUES, projection, aggregate and
window rows. The selected callers use the same fallible key construction for
sort, grouping, DISTINCT, set operators, aggregate DISTINCT/local ordering and
window partitions/peers/order. Recursive UNION borrows its declared output fields
when keying plain retained CTE rows. Bound JSONB equality continues through its
qualified scalar join fallback; the primitive equality extractor does not promote
JSONB to hash or merge joins.

## Evidence awaiting execution

- [SQL relational cases](../tests/sql_queries/ordering/jsonb_semantic_keys.rs):
  exact order/grouping, DISTINCT, sets, computed/stored/aggregate/window metadata,
  recursive UNION, lateral outer columns, aggregate DISTINCT, qualified joins and native SQL arrays.
- [Wire formats](../tests/pgwire_extended/types/json/semantic_keys.rs): JSONB
  parameter OIDs, text/binary input, DISTINCT identity and result OID/format.
- [Native document keys](../src/types/semantic/jsonb_tests.rs): exact Eq/Hash/Ord,
  signed decimal weights, measured admission boundary, cancellation after actual
  token work, last-clone lifetime and late numeric-error cleanup.
- [Decorated UNION ALL keys](../src/executor/execution/source_rows/jsonb_key_tests.rs)
  and [outer-column projection ownership](../src/executor/execution/jsonb_projection_tests.rs):
  measured admission boundaries, last-row ownership and late-error cleanup.
- [Typed carrier cases](../src/executor/semantic/tests.rs): TEXT contrast,
  SQL/document NULL, native/legacy/text SQL-array carriers, lower bounds and
  late-element-error cleanup.

Pinned PostgreSQL 18.6 witnesses and bounded Jev request/response records are
retained in ignored `target/v020-evidence/jsonb-semantic-keys/`. The first source
review found no concrete counterexample (none probability 0.74), with ownership
risk 0.23 and comparator risk 0.25. Its metadata probe selection (0.55) caused the
stored, aggregate, window and recursive UNION regressions to be added. The last caller review selected no confirmed counterexample (0.36, confidence
0.15), but assigned outer metadata 0.32 and decorated UNION ALL ownership 0.30;
the two focused native modules above were authored in response. They remain
unexecuted. Required build, locked tests, full pedantic Clippy, complete final
formatting/test policy and performance qualification remain pending on the final
combined head. Scoped source formatting, size checks and focused test-policy
analysis have passed; these checks do not execute the regressions.

## Remaining acceptance cells

This increment does not close #809. Existing generic numeric input can reject
`0.01e131073` in its dense-expansion preflight although PostgreSQL accepts its
131072-digit value; this producer boundary is separate from key normalization.
Recursive SEARCH/CYCLE lineage still uses its established untyped record keys;
JSONB-valued lineage needs an explicit type-aware follow-up before claiming that
cell. Persisted JSONB index comparison is outside this private runtime-key change
and must not be inferred from it. The parent bundle separately owns the remaining
operator/containment qualification and final lifecycle/performance evidence.
