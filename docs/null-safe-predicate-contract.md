# Finite null-safe query predicates

The user selected this finite #798 query/DML slice on 2026-10-08. It resolves
#792 only for this slice. Local implementation and focused qualification are in progress;
#798 remains open for excluded persisted-definition compatibility.

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

Preserve existing exact integer/mixed numeric equality, signed-zero equality,
canonical scalar/temporal carriers, recursive JSON equality and finite VECTOR
input/dimension rules. Existing compatible one-dimensional scalar ARRAY
equality distinguishes a SQL NULL array, an empty array and NULL element slots.
ARRAY(VECTOR) wire output and array-of-array grammar remain excluded. Declared
ARRAY metadata is not inferred from an arbitrary JSON shape. No existing `=`
law is changed to implement these operators.

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

Existing query controls, applicable admission, ownership and error ordering
remain in force. This slice does not claim bounded generic scalar expression
allocation, native CTE promotion or stronger statement visibility.

## Persisted-definition exclusion

Reject actual new operators with `CassieError::Unsupported` (SQLSTATE0A000):
`IS [NOT] DISTINCT FROM is not supported in persisted definitions; compatibility law is not selected`.
Validate before publication or row/catalog/object-ID/generation mutation.

Guard index expression/predicate ASTs; stored view SQL; function/procedure
bodies; materialized projection/version and rollup expression definitions;
and current CHECK/DEFAULT definition routes. Include supported direct durable
metadata admission, generated definitions and definition parsing/use after
restore/replay. Quoted/comment text is not an operator. Preserve existing
in-memory registration signatures and current CHECK/default grammar.

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

Use generic typed fixtures and an actual version-pinned PostgreSQL oracle only
for compatible cases; exact mixed/rich current laws use independent literal
expectations. Named existing tests, helper mappings and Jev judgments are not
acceptance. Focused RED/GREEN, paths, source/log hashes, remaining gaps and
required ordered gates are recorded with source/log hashes in isolated evidence.

On main `e3bc2974`, focused controls passed 38 integration tests (31 slice
controls and seven existing support controls), two unit controls and 280 wire
cycles in one wire test. The NULL-residual pagination control returned IDs3,5
while actual counters reported a six-row collection scan and zero keyset/index
seek scans; this is fallback evidence. Restored view/routine, loaded index/default
and projection-build/rollup-refresh controls reproduced and corrected rejection
gaps. Corrected fixture attempts and the unsupported NaN test assumption remain
in the evidence record; current FLOAT canonical NaN equality is preserved.

This is a focused checkpoint, not completed acceptance. Remaining adversarial
checks include malformed contextual literals and all supported restored version
repair/replay callers. Complete ordered build/test/Clippy/fmt/validators, review,
publication and exact-head hosted validation are pending. #798 remains open.
