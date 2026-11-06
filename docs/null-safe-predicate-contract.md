# Finite null-safe query predicates

The user selected this finite #798 query/DML slice on 2026-10-08 under the
former #792 planning program. That broad program was later closed as superseded.
PR [#867](https://github.com/cntryl/cassie/pull/867)
merged its finite implementation at `ad0a001a65fae23726cd184fedf1f973d649e60c`.
#798 is qualified by the unified query/array closeout for versioned definitions
and NULL-aware hash/merge keys. The earlier iteration records below remain
historical evidence; the 2026-11-05 implementation section governs current scope.

## Grammar and transient API

Add `BinaryOp::IsDistinctFrom` and `BinaryOp::IsNotDistinctFrom` using the
existing binary expression structure. Accept `lhs IS DISTINCT FROM rhs` and
`lhs IS NOT DISTINCT FROM rhs` at comparison precedence, below arithmetic and
above leading NOT/AND/OR. Existing tokens, separators, quoting, operands,
parameters and casts remain unchanged. Each operand evaluates once.

No IS TRUE/FALSE/UNKNOWN, row constructors, ILIKE/regex, new casts/types,
multidimensional ARRAY syntax or new wire identities are selected.

## Current equality and NULL law

Validate current declared operand compatibility before evaluating the NULL
truth table. Two SQL NULLs are not distinct; exactly one SQL NULL is distinct.
For two comparable nonNULL operands, use current predicate `=` equality and
its inverse. Do not replace it with grouping/DISTINCT/sort-key equality.
An unsupported nonNULL pair produces an error, never incidental NULL or an
invented Boolean. Existing cast/division/overflow/codec errors remain visible,
including a right-operand error when the left operand is NULL.

Preserve exact integer/integer equality, scalar/temporal carriers, recursive
JSON equality and finite VECTOR input/dimension rules. The current
[mixed numeric SQL comparison contract](mixed-numeric-comparison-contract.md)
records the selected integer/FLOAT operator input domain used by ordinary and
null-safe equality; the old #904 parent is closed and is historical provenance.
This null-safe surface remains Experimental while #798 and #761 retain broader
acceptance matrices. Signed zero and admitted nonfinite FLOAT inputs require
explicit operator oracle controls; general key semantics do not prove their SQL
behavior. Existing compatible one-dimensional scalar ARRAY equality
distinguishes a SQL NULL array, an empty array and NULL element slots.
ARRAY(VECTOR) wire output and array-of-array grammar remain
excluded. Declared ARRAY metadata is not inferred from an arbitrary JSON shape.
No other equality law or persisted-operator admission changes.

Result metadata is BOOLEAN OID16, typlen1, typmod-1 even for NULL operands.
ParameterDescription retains original bound OIDs. Existing input adapters
700/1700 do not acquire a new unconstrained numeric output ABI.

## Transient execution and planner legality

Permit existing SELECT projection/WHERE/HAVING/JOIN ON and DML expression
positions, including ordinary nested SELECT/CTE expressions. Preserve phase
order, bag multiplicity, NULL-safe joined pairs and pagination/keysets.

Initial execution uses existing scalar scan/residual fallbacks. Ordinary Eq
joins skip NULL keys and are not legal native null-safe joins. Null-omitting
scalar indexes cannot answer NULL/NULL equality. Independent eligible conjuncts
may still select existing candidate paths with an authoritative residual.
Encoded/typed paths decline unqualified null-safe kernels before conversion.
Installing an index or configuring workers does not prove dispatch.

### Native numeric null-safe kernels

The shared typed-batch capability authority admits `IS [NOT] DISTINCT FROM`
to the native expression kernel under the same rule as native `=`: both
operands are columns, parameters, numeric literals or SQL NULL, and both
resolve to SMALLINT/INT/BIGINT/FLOAT or NULL. The kernel applies the NULL table
first and then uses the same `compare_numeric_values` law as native `=`, so
NaN equals NaN, signed zero compares equal and integers stay exact. The result
is a non-NULL BOOLEAN and may feed native AND/OR. Typed aggregate predicates and
projected-scan field pruning admit the operators on the same terms. BOOLEAN,
text, JSON, ARRAY, VECTOR, nonnumeric parameters and binder-inserted mixed
integer/FLOAT input casts keep the scalar fallback, and mixed predicates keep
the same read path as their ordinary `=`/`<>` counterparts.

At the historical transient-only checkpoint, null-safe equality was not a hash,
merge or index join key. `JOIN ON
l.n IS NOT DISTINCT FROM r.n` keeps the scalar nested-loop fallback, and no
index answers a null-safe predicate. Those native join/index kernels remain
open #798 work.

Existing query controls, applicable admission, ownership and error ordering
remain in force. This slice does not claim bounded generic scalar expression
allocation, native CTE promotion or stronger statement visibility.

## Historical persisted-definition exclusion

Reject actual new operators with `CassieError::Unsupported` (SQLSTATE0A000):
`IS [NOT] DISTINCT FROM is not supported in persisted definitions; compatibility law is not selected`.
Validate before publication or row/catalog/object-ID/generation mutation.

Guard index expression/predicate ASTs; stored view SQL; function/procedure
bodies; materialized projection/version and rollup expression definitions;
and current CHECK/DEFAULT definition routes. Include supported direct durable
metadata admission, generated definitions and definition parsing/use after
restore/replay. Quoted/comment text is not an operator. Preserve existing
in-memory registration signatures and current CHECK/default grammar.
Nested SQL definition admission also propagates the exact selected parser
rejection from CHECK/default guards before publication. Other malformed or
unsupported stored SQL keeps its existing caller-owned diagnostics.

Opaque stored bytes are not rewritten; startup does not reject a keyword
substring. Existing definitions and no-op/authorization semantics remain
unchanged. Persisting the new enum variants or SQL operators requires a later
selected catalog compatibility/migration law; no such law is inferred here.

## Finite acceptance and evidence

NS01 covers syntax/precedence/camouflage/malformed operands and transient AST
roundtrip. NS02 covers legal scalar/NULL/once-per-operand/error behavior. NS03
covers supported ARRAY equality and text/binary BOOLEAN/parameter descriptors.
NS04 compares actual row/index/residual/keyset/join/overlay paths. NS05 covers
applicable controls, retained ownership and failure cleanup. NS06 rejects all
supported durable definition routes before mutation and preserves old goldens.

Earlier exact mixed/rich literal expectations remain bound to their original
finite baseline. The next selected mixed SQL operator law requires actual
version-pinned PostgreSQL outcomes and independent literal controls; old exact
mixed expectations cannot adjudicate that correction. Named existing tests, helper mappings and Jev judgments are not
acceptance. Focused RED/GREEN, paths, source/log hashes, remaining gaps and
required ordered gates are recorded with source/log hashes in isolated evidence.

On the isolated branch based on main `e3bc2974`, initial focused controls passed 38 integration tests (31 slice
controls and seven existing support controls), two unit controls and 280 wire
cycles in one wire test. The NULL-residual pagination control returned IDs3,5
while actual counters reported a six-row collection scan and zero keyset/index
seek scans; this is fallback evidence. Restored view/routine, loaded index/default
and projection-build/rollup-refresh controls reproduced and corrected rejection
gaps. Corrected fixture attempts and the unsupported NaN test assumption remain
in the evidence record; current FLOAT canonical NaN equality is preserved.

The final focused replay passed 41 integration controls (34 slice controls and
seven existing support controls). A supported loaded-version repair reproduced
a generic execution error instead of the selected unsupported-feature diagnostic;
a guard now runs before version mutation/persistence. Malformed contextual JSON
literals retain the existing binding error for ordinary and null-safe comparisons,
including SQL NULL rows; JSON null remains distinct from SQL NULL. The existing
Eq AST encoding golden is unchanged. Unsuitable fixture attempts remain separate
from the genuine repair RED/GREEN evidence.

The preceding paragraphs record focused iteration before PR #867's final
acceptance. That merged record reports all 20 ordered local stages passed,
3,633 passed/0 failed/9 ignored and all 13 hosted checks passed. Preserve those
source-bound results. They do not qualify the newly planned mixed SQL operator
correction or complete #798's remaining persisted/native obligations.


## Selected v0.2 persisted-definition expansion (2026-11-04)

The user approved this contract direction on 2026-11-04. It is selected
implementation scope, not implemented or qualified support. Existing rejection
and compatibility boundaries remain until the owning acceptance cases pass.

### #798

Authorize IS DISTINCT FROM and IS NOT DISTINCT FROM in the existing supported
CHECK/default, view, scalar-index expression/predicate, materialized-projection and
rollup definition contexts. Keep routine execution outside roadmap scope. Retain
once-per-operand evaluation, pre-NULL type compatibility checks, ordinary predicate
equality for non-NULL operands, BOOLEAN OID 16, exact integer behavior and selected
mixed primitive casts. Qualify a private hash-join NULL bucket and native comparison
kernels against scalar fallback. Null-omitting indexes must keep scan/residual fallback
for NULL matches unless a separately proven existing index can supply the complete
answer; do not invent a new index key encoding just to claim dispatch.

Version durable expression-definition records and preserve existing serialized
variants/goldens. Pin admitted operator semantics to the documented contract version,
validate ASTs on create/load/restore, and reject unknown future semantics. Rebuild
derived indexes from authoritative rows; failed or rolled-back admission leaves no
definition/index/name reservation. No new value OID or wire codec.

The earlier persisted-definition exclusion describes current runtime behavior.
Its replacement is selected above, but admission stays rejected until the new
versioned contract and its create/load/restore/rollback tests are implemented.
No index key encoding or stored-program execution expansion is selected.

## Unified closeout implementation (2026-11-05)

The selected expansion now has source implementation in the unified query/array
branch; executable acceptance remains deferred until its complete source and Jev
review are frozen. Definition records for views, scalar indexes, constraints,
materialized projections and rollups carry `definition_semantics_version`.
Missing/zero retains legacy rejection; version 1 admits the selected operators;
unknown future versions fail closed. Zero is omitted during serialization, so
legacy record and AST goldens retain their encoding. Direct metadata callers
explicitly select version 1. This does not promise downgrade support in binaries
that predate the versioned contract.

Simple field/literal CHECK comparisons admit both operators, distinguish SQL
NULL from JSON null, and validate existing as well as new rows. Constant DEFAULT
comparisons fold to the existing literal carrier while retaining their expression
and version for admission and catalog description. Arbitrary expression defaults
and general CHECK ASTs retain their independent owners. Table cloning and ALTER
DEFAULT preserve/select the definition contract; legacy stored procedures still
reject nested uses of the new operators.

Loaded INNER/LEFT numeric/Boolean hash joins and scalar merge joins carry an
explicit NULL-matching flag. The NULL bucket retains duplicate multiplicity and
outer extensions under existing admission and cancellation controls. Ordinary
`=` retains NULL exclusion. Bounded/index join candidates decline null-safe keys
before loading; null-omitting indexes keep authoritative residual/fallback reads.
No index encoding or public AST variant changes are needed.

Every version-1 definition requires the existing v0.2 reader marker before
publication, including a version-1 body containing only older operators. Public
metadata writers enforce this independently; SQL creation fences before view
object IDs or materialized output are allocated. Legacy version-zero metadata
and merely reopening a v1 store keep their prior behavior. An acknowledged
marker can remain v2 after later feature failure, as required by the monotonic
[storage compatibility transition](storage-compatibility-transition.md).

The existing transactional-DDL profile is unchanged: table CHECK/default changes
and scalar indexes use the current staged catalog/data overlay and roll back
without retaining their names. CREATE VIEW, materialized-projection and rollup
commands retain their existing active-transaction rejection; they do not acquire
new transaction support through this operator expansion. These boundaries and
the broader #833 owner remain separate from accepting this finite increment.
Direct version-1 constant-default metadata must supply its bound literal value;
an unbound expression is rejected before publication instead of silently omitted.


Unknown string literals in selected primitive null-safe comparisons inherit the
numeric or BOOLEAN peer signature before the NULL table is applied. Incompatible
typed primitive families fail with 42883 even when a typed operand is NULL. This
qualification is shared by transient queries and constant DEFAULT folding;
ordinary comparison signatures and character trailing-blank rules are unchanged.
The shared qualifier's Jev gap 0.41 requires the retained both-order, invalid
BOOLEAN/integer, typed-NULL, unknown-BOOLEAN and wire-recovery probes. Fresh
PostgreSQL 18.6 signature receipts provide their value/error oracle.


### Bound simple CHECK comparisons (review correction)

The numeric CHECK probe exposed a raw-JSON equality mismatch for BIGINT 7 versus
literal 7.0. Jev selected (1.0) reuse of the ordinary binder and evaluator over a
prepared simple comparison, preserving the original field/operator/literal
carrier. Version-one CHECK records may carry an optional boxed bound expression;
legacy records omit it and retain their bytes. Preparation accepts only the
existing simple shape. Admission validates the same owning column, literal and
null-safe operator, with only binder-added casts/private float wrappers; it does
not admit functions, subqueries, additional fields or general CHECK expressions.
Non-NULL version-one CHECK metadata requires the prepared comparison. NULL-only
metadata retains its simple NULL table. New/existing-row enforcement uses the
same comparator as queries, including exact BIGINT, selected FLOAT input-domain
coercion and unknown literals. The existing metadata snapshot accounting owns
the bound tree; evaluation retains reached scratch under the originating request
controls. No per-row SQL binding or additional storage interface is introduced.
