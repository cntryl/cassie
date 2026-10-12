# Primitive mixed numeric SQL comparison contract

This defines the selected finite [#904](https://github.com/cntryl/cassie/issues/904)
bundle on accepted #936, including its private implementation and required
qualification. Retained PostgreSQL 18.6 baselines, behavioral RED evidence,
native comparisons, adversarial probes and the complete ordered local gate
govern acceptance. This finite contract does not establish full type-registry
or PostgreSQL parity support.

## Operator input domain

For existing declared SMALLINT, INT, BIGINT and FLOAT (float8/OID701), resolve the
actual built-in comparison signature and its implicit casts before evaluating
`=`, `<>`, `<`, `<=`, `>` or `>=`. Use the pinned #793/#904 PostgreSQL cast and
operator inputs, rather than a generic common-result numeric ladder. Two integer
operands retain exact integer comparison. An integer/FLOAT pair uses the selected
float8/float8 operator input domain, in both operand orders; its temporary integer
to float8 conversion may round. Stored integers and output payloads stay exact.

The accepted transient `IS DISTINCT FROM` and `IS NOT DISTINCT FROM` forms use
that resolved equality for comparable nonNULL operands. Preserve declared-family
validation, the two-NULL/one-NULL truth table, once-per-operand evaluation, error
order and BOOLEAN metadata. This supersedes only the earlier exact *mixed SQL
comparison* wording in #798 and the null-safe/typed-join contracts. It does not
change their other type-family laws or approve persisted null-safe operators.

Known parameter input identities remain their declared OIDs even when the
selected operator converts an input expression. Typed NULL retains that identity.
Resolve from declared/scoped types, not a sampled runtime carrier or first
nonNULL row. Unknown/polymorphic overload resolution, user-defined operators,
NUMERIC and new FLOAT4 behavior are separate obligations.

## Joins and access paths

INNER and LEFT pure equality joins use one selected comparison domain for both
build and probe keys. Preserve duplicate multiplicity, NULL-key exclusion, outer
extensions and original payload values. Loaded typed, bounded, streaming and
merge paths must produce the same operator result or decline before partial
output or lossy pruning. A generated CAST that removes native-key recognition
must not be reported as qualified native typed/hash/merge dispatch. Actual
completed-path witnesses distinguish a retained optimized path from fallback.

A FLOAT comparison against a BIGINT index may match more than one adjacent
integer key after conversion. One encoded point lookup is insufficient unless
its completeness is proved. Keep existing exactness guards, or use an explicitly
observed authoritative scan/residual fallback; do not remove a guard merely to
make the probe eligible. No stored index encoding or durable key layout changes.

Preserve safe FLOAT field-index reads for comparisons with integer literals.
Recognize only the paired binder-generated input conversions, materialize the
integer bound in the selected float8 domain, and verify the indexed field's
catalog type is FLOAT before reading keys. Explicit casts, parameters and other
constant expressions do not acquire eligibility from this recognition. Preserve
signed-zero and transaction-overlay fallbacks. A manually constructed public
AST with the same wrapper shape must still decline an integer-field index.

Existing transient query/DML positions follow the selected operator law. This
finite operator acceptance does not qualify stored-definition compatibility,
unique/FK equality, exclusion constraints or concurrent enforcement. #769 CON-05
and the full #798/#904 criteria remain open for their actual remaining matrices.

## Preserved boundaries

The selected derived-source comparison caller also requires exact lookup of
synthetic precomputed expression keys before evaluating its expression. The
fresh 20-target provenance baseline at test-only commit
`d73f7b636a2bbc8402c17ae1d95361d83c826079` retained 60 native paths. Two derived
comparison projections emitted their source decimal values under BOOLEAN
descriptors; binary output failed with `08P01`. Source inspection traced this to
the projection shortcut resolving an arbitrary expression key by its final
column-name suffix. Include the focused private projection correction and both
exact PostgreSQL query oracles in this bundle, alongside the type-origin repair.
This is a prerequisite from #761's existing relational-correctness scope. It
changes no public row-lookup API or persistent format. Its failed baseline stays
retained, and qualification still requires fresh successful observations.

Do not change `compare_numeric_values`, `SemanticNumber`, `SemanticKey`, general
grouping/DISTINCT/set/window keys, recursive JSON equality, ARRAY rules or exact
integer carriers to implement this operator correction. IN, BETWEEN and simple
CASE retain their current laws and are explicit no-drift controls unless a later
finite selection includes them. Existing integer output/codec exactness guards
are distinct from a temporary selected operator-input conversion.

Signed zero and every currently admitted nonfinite FLOAT input need explicit
PostgreSQL oracle controls. In particular, the signed-NaN branches in the general
exact comparator do not prove float8 SQL NaN equality/ordering. Preserve those
general-key branches; adjudicate operator-local behavior from actual reference
outcomes rather than asserting equivalence by reuse.

## Prerequisites, history and acceptance

The completed private function-input foundation is [PR #935](https://github.com/cntryl/cassie/pull/935),
commit `3304f103e6484102b3c60588469009ee240baa7e`; its 156 selected PostgreSQL 18.6
comparisons and 3,963/0/9 full gate qualify only that finite prerequisite. The
completed occurrence foundation is [PR #936](https://github.com/cntryl/cassie/pull/936),
commit `c187ccd5e27786451f112333654df2d40b91788c`, whose tested and merged tree is
`28f40bf269f37232da5c7cb0ca5f61c905ecb06d`. Its actual full ordered local gate
passed 3,991/0/9 across 35 groups/34 binaries, all 28 commands and 21 touched-test
policies; [exact-head review 5486354702](https://github.com/cntryl/cassie/pull/936#pullrequestreview-5486354702)
and merged-source readback accepted that finite slice. Both commits must remain
completed staged prerequisites before this dependent runtime work. Their wider
registry/coercion/catalog/resource owners and original full-owner .85/.72 gaps
remain open; neither already implements this mixed operator law.

PR [#867](https://github.com/cntryl/cassie/pull/867) merged the finite null-safe
query/DML slice at `ad0a001a65fae23726cd184fedf1f973d649e60c`. Its final record
reports 3,633 passed, zero failed and nine ignored, all 20 local stages and all
13 hosted checks passed. Its exact-mixed literal expectations remain historical
evidence; they are not the oracle for the new mixed operator domain. Persisted
null-safe compatibility and native null-safe kernels remain unqualified.

#869 is closed at PR #884, commit
`4a35dc373c4ac449184d42001055b447b3c53dc4`; #760 is closed at PR #862, commit
`e3bc2974da6f8f5c60cc73afa9f3c94553dfeee6`. Their finite repairs remain accepted.
The historical #904 BIGINT/FLOAT scalar and join blocker feeds #761's remaining
relational integration; it does not reopen #869 or #760.

Required fresh acceptance includes all six comparisons in both operand orders,
2^53 boundaries and integer extrema; exact integer/integer and declared FLOAT
controls; NULL, signed zero and admitted nonfinite inputs; known parameter and
typed-NULL input descriptors; simple/extended text/binary paths; scalar, typed,
indexed/fallback and transaction-overlay twins; INNER/LEFT duplicate bags; and
transient null-safe equivalence. Use aliased output columns to separate the
existing output-name owner #924. Reconcile any old test oracle only after its
exact reference query, seed and result have been retained.

### Required transient DML qualification

The same selected operator law applies to existing transient UPDATE/DELETE/INSERT,
ON CONFLICT and RETURNING expression positions, including WHERE/SET and existing
admitted INSERT source/value routes. Query/join observations cannot qualify these
mutation callers. Before implementation, retain focused PostgreSQL/native DML
observations with checked reset/setup; derive Rust RED expectations from their
exact query/seed/results. Use fresh per-case rows containing the 2^53-adjacent
BIGINT pair, a NULL control and exact integer/FLOAT controls. Candidate forms
below identify the acceptance routes; they are UNRUN inputs, not passing results:

```sql
UPDATE mixed_dml
SET matched = k = CAST(9007199254740992 AS FLOAT)
WHERE k >= CAST(9007199254740992 AS FLOAT)
RETURNING id, k, matched;

DELETE FROM mixed_dml
WHERE k = CAST(9007199254740992 AS FLOAT)
RETURNING id, k;

INSERT INTO mixed_dml(id, k, matched)
VALUES (4, CAST(9007199254740993 AS BIGINT),
        CAST(9007199254740993 AS BIGINT)
        IS NOT DISTINCT FROM CAST(9007199254740992 AS FLOAT))
RETURNING id, k, matched,
          k = CAST(9007199254740992 AS FLOAT) AS ordinary_equal;

INSERT INTO mixed_dml(id, k, matched) VALUES (1, 0, FALSE)
ON CONFLICT (id) DO UPDATE
SET matched = k = CAST(9007199254740992 AS FLOAT)
WHERE k = CAST(9007199254740992 AS FLOAT)
RETURNING id, k, matched;
```

Adjudicate all six comparisons and transient null-safe equivalence across the
applicable existing mutation positions, both operand orders, NULL and known
parameter twins. Retain exact affected counts, RETURNING values/descriptors,
command tags, statement error/atomicity ordering and committed or staged state;
include transaction-overlay and simple/extended text/binary witnesses. Prove
original BIGINT payloads remain exact after mutation and preserve once-per-input
evaluation. An unsupported existing form remains an open obligation with its
current owner; do not silently substitute a SELECT, drop a DML route or change
stored-definition/index/constraint support to make the matrix pass. This bundle
does not complete #769 CON-05 or authorize persisted null-safe operators.

Follow the complete ordered local gate, independent adversarial review and every
Jev gap probe at or above 0.30. Record actual required GitHub rules, squash and
read back the merged commit/issues. A successful finite bundle cannot close
#904/#761/#798/#769/#793/#791/#848/#849 without each owner's full acceptance.
