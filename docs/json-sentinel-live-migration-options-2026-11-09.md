# #809 ordinary JSON object: live representation and compatibility decision

Status: decision input, not an activated migration. Current source baseline
`541a9e7d869a51e1c2b3730c9947fcbb4db63aa1`; no Cassie build or test was run.
The linked isolated test-first witnesses do not constitute execution evidence.
PostgreSQL 18.6 observations and the initial cross-entry policy are recorded in
`json-private-sentinel-semantics-2026-11-09.md`; the earlier representation and
wire constraints are in `json-ordinary-representation-options-2026-11-09.md`.

## Observable requirement and current paths

An ordinary JSON object containing `$serde_json::private::Number` or
`$serde_json::private::RawValue` must remain an object at every nesting depth.
JSON and JSONB retain their distinct parse/format/duplicate/error contracts,
SQL NULL remains distinct from document null, JSONB rejects decoded NUL with
`22P05`, exact numeric values and overflow diagnostics remain correct, and
JSON/JSONB wire OIDs remain 114/3802 (binary JSONB version byte 1). Direct casts,
text and binary Bind, text and binary COPY, insertion, CRB row encoding and
projected/full scans, operators, comparison/grouping and result transport must
agree. Do not accept an object on one entry only to silently turn it into a
number or another document at a later boundary.

On this baseline, JSONB SQL casts call `types::jsonb::canonical`, which calls
serde Value deserialization; extended text Bind and binary JSONB codec validate
then parse again into serde Value. Controlled binary COPY has a separate arena
that intentionally retains Value-mode sentinel behavior. Row-blob field type
`TYPE_JSON` (0x07) stores serde JSON bytes, and full and projected decoders
reparse those bytes into serde Value. `JsonbKey::from_text`, existence,
JSONPath and other consumers have additional reparses. A plain-object policy
in just one parser therefore cannot close this issue.

The Midge boundary is direct: Cassie's public adapter `Midge::put_document`
takes a `serde_json::Value`, and public `DocumentRef.payload` is also a serde
Value; published cntryl-midge 0.5.2 byte APIs do not by themselves change those
Cassie adapters or their index, mutation, column and scan consumers. The public
`Value::Json` variant contains `serde_json::Value`, `Value` derives serde and
Clone, and `QueryResult::rows()` exposes `&[Vec<Value>]` without a fallible
materialization step. There are hundreds of existing source matches and user
construction can use `Value::Json(serde_json::json!(...))`. Any change to the
variant, exposed payload or exhaustive variants is a source-level API break,
not an invisible private implementation detail.

## Proposed explicit alternatives

1. **Replace the public document representation.** Introduce a Cassie-owned
   immutable JSON document whose controlled storage/visitor is the common
   representation for SQL cells, public `Value::Json`, adapter documents and
   semantic keys. Provide explicit fallible conversion methods for callers who
   need a serde Value, and migrate all constructor/match/accessor and Midge
   adapter callers. This breaks current `Value::Json(serde_json::Value)` source
   construction and `DocumentRef.payload` expectations; conversion cannot be
   silently performed by the infallible `QueryResult::rows()` accessor. Its
   explicit public migration/version policy, equality, Clone, serde shape and
   error behavior require selection before code. A query-owner-coupled document
   cannot be treated as freely cloneable until its shared allocation, parent
   tracker, foreign tracker and final deallocation are proven.
2. **Add a native public document variant alongside the old variant.** Retain
   existing `Value::Json` construction and old adapter APIs while exposing a
   Cassie-owned document variant and controlled new document accessors. This
   still breaks external exhaustive matches; it also creates two representations
   for identical JSON and changes what `rows()` returns for selected data.
   Cross-variant equality/hash/order, serde representation, direct `as_str`,
   arrays, casts, result exposure and compatibility conversion must be defined
   explicitly. Keeping a legacy adapter cannot establish correct ordinary
   objects in a serde-only returned `DocumentRef` unless that conversion is
   separately proven; returning a SQL String is not equivalent.
3. **Keep current APIs and construct serde Values.** A plain arena can decide
   document semantics, but producing `serde_json::Value::Object` still uses
   serde's public `Map`, backed by `BTreeMap` without a reserve or bounded
   comparison/cancellation hook; arbitrary Number's owned-string constructor
   is explicitly not public API, and public `FromStr` still lacks a proven
   bounded-copy/peak contract. A lease covering only the source arena does not
   admit a separately built Value. This is not a proven fully controlled path.
4. **Private wire-only path.** A schema-typed raw/arena cell can in principle
   carry correct bytes through a private pgwire frame; it cannot make existing
   embedded `QueryResult::rows()` or public `DocumentRef` return a matching
   `serde_json::Value` by itself. It is a partial feature only, and unsupported
   public paths must not silently claim an equivalent value.

A native document type also needs an explicit serde compatibility design.
With arbitrary precision enabled, serde_json 1.0.154 sends sufficiently wide
numeric tokens to a visitor as a synthetic map containing its private Number
key (`ParserNumber::String` / `NumberDeserializer`); ordinary JSON objects can
contain the same key. A generic `Deserialize` visitor that treats every such
map as an ordinary object would therefore misread wide numbers, while blindly
following the sentinel loses genuine objects. The existing raw input arena can
distinguish lexical token kind, but generic serde deserializers and the existing
public serialized `Value` shape require a separate, source-proven policy rather
than assuming a derived visitor solves both cases.

All candidates need a single source-owned JSON representation through exact
input validation, DML/identity/constraints/indexes, write publication through
existing Midge APIs, projected/full scans, expression/aggregate/key construction,
result/portal/REST and compatible public access. No Midge engine source change
or additional storage layer is selected. Use separate checked/owned builders
and explicit destination/source overlap; parsing, sorting, payload ownership,
allocator transient capacity and teardown need their own proofs. No arbitrary
size shortcut or hidden serde API establishes those guarantees.

## Existing bytes and old readers

The current row writer emits CRB1 `FORMAT_VERSION = 1` and `TYPE_JSON` bytes.
A new reader can interpret a genuine retained object from those bytes as an
ordinary object while leaving all other valid legacy documents unchanged, but
it cannot infer lost input: a legacy sentinel object already transformed to
numeric `7` and an original numeric `7` can produce byte-identical rows. The
isolated row test encodes that counterexample. No automatic restoration may be
claimed; repair requires external source evidence.

Keeping the same type/version means an old Cassie reader may still silently
reinterpret newly preserved sentinel objects. Alternatively a new selected CRB
version or field encoding could force old readers to reject: current
`RowDirectory` rejects an unknown `FORMAT_VERSION`. Such a format/version change
is a separate migration, not implied by this document, and requires explicit
new-write/old-read, downgrade, restart, rollback and mixed-version policy. In
both choices the new reader must preserve valid preexisting bytes and document
irreversible ambiguity. Midge remains the sole engine and row storage layer.

## Evidence required before activation

The unexecuted tests in the isolated source lane cover sentinel keys as first,
nested and escaped duplicates; SQL casts/extraction/existence; JSON and JSONB
text/binary Bind; text/binary COPY; fresh persisted rows, restart and semantic
key parity; distinct SQL/document null; malformed/NUL/numeric error cases; and
identical transformed-versus-scalar legacy bytes. A production candidate must
add public API/serde and version-specific old-reader witnesses, ordinary control
objects, true object-vs-scalar equality and order, full/projected/index/column
scans, actual-work cancellation and exact/below-budget owner/drop probes. The
oracle/error expectations must be independently verified, and all checks run in
the required deferred sequence before issue completion.

## Selected design direction and unresolved prerequisites

Jev selected option 1 as a **documented design direction only** (0.84; combined
API/storage/owner lead 0.92), receipt
`target/assembly-review/json-public-api-migration-{request,response}.json`.
This selection is not runtime approval, a semver decision, a materialization
proof or a Midge fencing capability. Before changing the public API or row
format, specify the exact compatibility conversion, `Value`/`DocumentRef`
source and serde migration, Clone/query-owner behavior, every public result and
Midge byte boundary, and an explicit mixed-reader upgrade/downgrade fence.
Old readers do not automatically consult a new Cassie metadata marker. A row
format bump can make the current row decoder reject an unknown version, but
all write/read, projected/column/index, old-reader and recovery routes must be
source-proved before relying on it. Do not write the new format or publish a
new API merely because this design direction was selected.

The representation must say which guarantees hold for bare JSON spelling and
duplicates versus JSONB canonical form and last-key semantics, how exact
numbers and JSON null differ from SQL NULL, and how consumers distinguish them
without the schema information that `Value` alone currently lacks. Generic serde
conversion must address the arbitrary-precision sentinel map issue described
above. Preserving an infallible public Clone by deep-copying unadmitted buffers
would violate resource ownership; a shared owner must also cover allocation and
final deallocation, including its header. A fallible `try_clone` alternative is
a further source compatibility change and requires a migration for existing
Clone consumers. No one of these options is yet proven complete.
