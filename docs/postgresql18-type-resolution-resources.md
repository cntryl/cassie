# PostgreSQL 18 type resolution and resource contract (historical)

> **Historical contract proposal for the retired full-parity target.** It is
> not the current runtime type registry or resource contract. Current behavior
> is documented in [Feature Support](feature-support.md),
> [Type Contract](type-contract.md), and
> [Performance Contracts](performance-contracts.md).

This record describes the former proposed shared type registry. It is not
current implementation/support evidence. PostgreSQL 18.6 source archive
`555610c24d53e4316da5b7d3fc25c279d96856d5e0e23ee308c328c5fa881d9f`
was the immutable reference for that proposal; source-file hashes accompany the
registry inputs.

The exact [catalog/export schemas](postgresql18-catalog-export-proposal.md)
were explicitly selected separately on 2026-10-10. Their retained approval
snapshot fixes the reviewed bytes. Selection authorizes affected implementation
after recorded prerequisites, without claiming runtime qualification. #793
remains open for staged owner records and actual acceptance.

## One resolver and catalog authority

Binding, execution, scalar/typed kernels, literals, parameter decoding,
RowDescription/ParameterDescription, text/binary output, catalog relations and
prepared plans use the same registered type descriptor and resolution result.
No parallel OID formula or convenience widening can override it. Retain the
existing Rust pipeline and extend its public AST/value types through focused
modules; #904 extracts oversized catalog code before extending it.

The registry key is database/catalog context plus stable type ID and immutable
descriptor revision; wire OID is a property and lookup index, not the durable
identity. Resolution additionally records typmod, collation, coercion context,
selected routine/operator identity and result descriptor. Polymorphic call
identities may be transient but their resolved actual argument/result types are
exact. Cache keys include relevant session/search_path/security/catalog and
collation/provider settings. Cached plans must revalidate or invalidate after
semantic dependencies change.

## Exact coercion inputs and algorithms

The immutable `pg_cast.dat` rows define source/target/routine, context `i`
(implicit), `a` (assignment), `e` (explicit), and method `f` (function), `b`
(binary), `i` (I/O). The registered built-in coercion graph must preserve all
235 rows, not reduce them to a single integer→numeric→float ladder. User-defined
casts/conversions #911 extend this graph transactionally. `pg_proc.dat` and
`pg_operator.dat` supply actual signatures; type category/preferred fields,
variadic/default argument forms and namespace visibility are part of resolution.

Port the semantics of these pinned source entry points into the shared Rust
resolver, with differential tests; do not embed a PostgreSQL runtime:

| Context | Normative source and entry points |
| --- | --- |
| legal coercion path | `parse_coerce.c`: `can_coerce_type`, `find_coercion_pathway`, `coerce_type`, `coerce_to_target_type` |
| CASE/UNION/VALUES/array/common result | `parse_coerce.c`: `select_common_type`, `select_common_typmod` and the callers in `parse_expr.c`/`parse_clause.c`/`parse_collate.c` |
| function/default/variadic/overload | `parse_func.c`: `func_get_detail`, `func_match_argtypes`, `func_select_candidate` |
| operator exact/candidate/unknown arguments | `parse_oper.c`: `binary_oper_exact`, `oper_select_candidate`, `oper`, `left_oper` |
| polymorphic arguments/results | `parse_coerce.c`: `check_generic_type_consistency`, `enforce_generic_type_consistency` |
| parameters and conflicting inference | `parse_param.c`: variable parameter reference/coercion hooks and `check_variable_parameters` |
| declarations/type aliases/modifiers | `parse_type.c`, `gram.y`, `pg_type.dat`, each `typmodin/typmodout` function |
| collation derivation/conflicts | `parse_collate.c`: `select_common_collation` and actual collation catalog/provider metadata |

Names in this table are checked against the source index before implementation;
any historical name mismatch is corrected in the record, not used to invent an
algorithm. Common-type resolution retains an identical domain when PostgreSQL
does, otherwise flattens domains as the normative context requires. Unknown
literals and all-unknown result lists select text only in the contexts where
PostgreSQL does. CASE's ELSE candidate order, pairwise UNION resolution and
array common element typing are observable laws. Typed NULL participates in
resolution; a value being NULL does not erase its declared type.

`anyelement/anyarray/anynonarray/anyenum/anyrange/anymultirange` enforce exact
family relationships; the `anycompatible*` families use their own compatible
common-type rules. Arrays, ranges and multiranges retain their registered
element/subtype identities. record/void/unknown/internal are contextual types,
not wildcard stored values. A result cannot infer its type from whichever branch
or row happens to execute first.

Parameters with supplied nonzero wire OIDs resolve through that type identity;
zero OIDs request PostgreSQL inference. The inferred descriptor is fixed before
Bind; decoded data cannot change it. Prepared statement/portal descriptors use
the actual PostgreSQL domain/base flattening rules and stable named OID mapping.
Missing/ambiguous/conflicting casts, operators, routines, parameters and collation
produce the reference SQLSTATE and preserve transaction/savepoint recovery.

## Encodings and byte identity

Full PostgreSQL SQL includes its legal server/client encodings and conversion
errors, not an implicit UTF-8-only exclusion. Store SQL text as validated bytes
with database encoding authority; SQL_ASCII non-ASCII bytes remain uninterpreted.
Do not force these values, SQL identifiers, enum labels or routine source into
Rust String or a UTF-8 JSON string. Catalog/export envelopes remain UTF-8 JSON
and carry SQL byte strings as `{encoding, bytes_base64}`. Lookup/length/collation
semantics follow the actual database context; global catalog names preserve
original bytes and use the PostgreSQL caller-context behavior, not invented
Unicode normalization or automatic cross-database transcoding.

Textual PostgreSQL binary send routines can use `pq_sendtext`, so binary format
is not automatically encoding-independent. Public output uses the actual
client_encoding. Durable tag-1 serialization fixes client_encoding to the
recorded database encoding and invokes the selected send semantics; decode uses
that same context. XML declarations and JSON/JSONB/jsonpath/enum/composite text
follow their actual codecs. Protocol containers build element send payloads in
the client context; private durable containers build them in the frozen database
context. Reserve conversion work and match unrepresentable/invalid encoding
errors. #904 owns the byte-capable value/registry foundation; #821 sessions and
#817 catalogs qualify settings/discovery, with database lifecycle ownership
recorded before its runtime.

## Scalar and container codec admission

Wire receive and durable decode are different contracts. Wire paths implement
PostgreSQL's accepted/noncanonical inputs and normalization. Durable payloads
are validated canonical registered send/container representations and consume
their complete frame. For example, Boolean binary input byte 2 is legal true
when PostgreSQL accepts it; durable Boolean send bytes are 0/1. Numeric receive
can normalize/truncate a legal incoming representation; durable output is the
actual resulting numeric_send bytes. Do not reject extra legal input forms by
applying stricter durable rules to the wire path.

Text/binary capability is explicit per type and function; an internal dummy
receive function is not a working public constructor. A container needing an
unavailable element binary codec returns the reference error, not text bytes
inside a binary frame. The private ACL durable codec does not fill its missing
public binary ABI. Named wire payloads use current stable OIDs where PostgreSQL
includes them; durable payloads use the retained enclosing descriptor instead.

CRB3's directory contains exactly one entry per present non-NULL field, sorted
by field_id, and no entries for absent or SQL NULL fields. Presence and NULL
bitmaps are authoritative for those two states; every NULL bit requires a
presence bit. `bitmap_len = ceil(max_field_id/8)`, unused high bits are zero,
and field IDs are one-based. Non-NULL payloads form a gap-free partition of the
remaining payload bytes, including legitimate zero-length scalar values.
Unknown or absent schema fields, mismatched codecs/versions, duplicate IDs,
wrapped offsets, NULL directory entries, overlaps/gaps and trailing bytes fail
before publication. This completes the approved bitmap/no-NULL-payload framing
without adding a fabricated NULL type codec. Domain NULL constraints are still
evaluated on admission. Recursive nullable framing uses state 0 alone for SQL
NULL; state 1 requires the approved tag/version/length and exact payload.

## Logical bounds versus operational admission

PostgreSQL type bounds define logical admission. Statement configuration and
available memory independently define whether an operation can acquire the
resources needed for an otherwise valid value. A smaller default query budget
does not reduce the documented final type domain; final qualification includes
profiles capable of exercising PostgreSQL boundary values.

| Family | Logical/reference bound |
| --- | --- |
| integer | exact signed 16/32/64-bit ranges |
| FLOAT4/FLOAT8 | IEEE binary32/binary64 plus PostgreSQL nonfinite/overflow/underflow laws |
| NUMERIC | explicit p 1..1000, s -1000..1000; unconstrained 131072 integral and 16383 fractional digits; exact operation-specific result bounds |
| character declarations | PostgreSQL `MaxAttrSize` and actual typmod input routines, character counts separate from UTF-8 bytes |
| bit declarations/values | `varbit.c` declaration limit and checked signed bit-length/value storage bounds; no arbitrary bytea alias |
| arrays | MAXDIM=6, signed i32 lengths/lower bounds and checked product/upper bounds; PostgreSQL `MaxArraySize`/allocation limits |
| dates/temporal | actual `date.h`/`timestamp.h` ranges, precision 0..6, epoch/sentinels and interval field bounds |
| strings/containers | actual PostgreSQL `MaxAllocSize`/family headers and value limits, with every encoded/decoded/work allocation checked independently |
| named type/object identity | selected monotonic u64 IDs and reserved/exhaustion-checked u32 wire identities |
| row/schema framing | u32 schema_version/max_field_id/count/offset/length; bitmap and total row size checked against representable lengths and accounted memory; never wrap |

The immutable source constants are recorded by name, value and file hash in the
contract inputs. They are not host `usize` guesses. On 32/64-bit builds, reject
conversion overflow before allocation; no unchecked multiplication, saturating
length that hides overflow or allocator panic is a valid admission path.

Use `QueryExecutionControls` and its shared memory tracker for parse buffers,
decoded values, container directories, traversal stacks, coercion/format work,
aggregate states and retained output. Reserve the actual requested allocation
before ownership, account capacity growth/copies, transfer/release ownership
exactly once and expose accurate peak/current counters. Cancellation checks
occur before acquisition, at every bounded chunk or container element batch,
and before publication. A chunk is at most 4096 input/output bytes or 256
container elements, whichever bound is reached first; long arithmetic/XML/path/
regex/library calls require an interruptible owner strategy and separate probes.
Use an accounted explicit traversal stack for nested values instead of native
recursion whose unchecked stack can crash. Statement deadline and cancellation
release every reservation/lock/reference on error and rollback/savepoint abort.

## Error and atomicity laws

Preserve exact PostgreSQL errors for semantic/input/type/constraint cases.
Typical owners include 22P02 invalid text, 22P03 invalid binary, 22003 numeric
range, 22023 invalid parameter/typmod, 42804 mismatch, 42846 impossible cast,
42883 missing routine/operator/codec, 42P18 indeterminate parameter and 23514
domain CHECK. Different malformed inputs may select different SQLSTATEs: the
pinned oracle fixture determines the actual case, not this illustrative list.

Operational memory admission reports 53200, selected program/value limits 54000
(or the exact applicable reference subclass), cancellation/deadline 57014.
Authoritative durable corruption is a data-corruption error and blocks serving
the affected state; it is never NULL, a skipped row or a successful empty result.
The pgwire error owner qualifies these mappings rather than matching text alone.
Cancelled/failed DML leaves no partial published row/catalog/index mutation.
If commit wins the cancellation race, report the committed outcome under the
written transaction/wire contract; do not return false rollback success.

## Required acceptance receipts

Each owner records source revision/reference artifact/provider/configuration,
concrete SQL/bind bytes and every applicable scalar/typed/index/overlay/simple/
extended/text/binary cell. Test values/multiplicity/declared order/NULL/coercion,
OIDs/typmods/parameters/SQLSTATE/tags, concurrent publication/privileges/locks,
rollback/savepoint/cancellation and exact restart/upgrade round trips. Clock,
random and physical-plan assertions are explicit semantic assertions.

Reference fixtures and durable goldens start the corpus; they do not establish
implemented runtime, complete statement/overload/context coverage or final
compatibility. Failed/skipped/unavailable/pending/unsupported in-scope cells kept
their owners open under the former program. That program was later closed as
superseded; #791 is not a current release gate. See the active roadmap for scope.
