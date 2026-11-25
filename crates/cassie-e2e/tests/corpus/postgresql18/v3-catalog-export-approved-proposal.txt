# Concrete v3 catalog and logical export schemas

Status: proposed remaining persistent catalog/export details for #793. The user
already approved CRB3 framing, durable/wire type identity separation, codec tags
1–9 and lossless export/recreate/reimport on 2026-10-10. This document does not
reopen that direction. It specifies the metadata bytes and export envelope that
the approved common proposal deliberately left to the per-family records.

Source baseline: merged #877 `2345e9025d937b6479ee5bdd6e13091537292e5f`;
current `src/catalog/metadata.rs`, `src/midge/adapter/metadata.rs`,
`src/midge/row_blob.rs`, `src/types/schema.rs` and #793. Existing metadata uses
JSON over Midge. Keep that organization and direct Midge ownership.

## Recommended remaining selection

Use versioned UTF-8 JSON catalog records, with explicit field names and numeric
identities encoded as decimal strings. Use the same semantic record schemas in
the versioned NDJSON logical export. Catalog record bytes never contain Rust
enum discriminants or serialized Rust ASTs. Authoritative values remain the
approved typed payloads; JSON is an envelope, not an exact-number value carrier.

## Canonical envelope

Every authoritative metadata record is:

```json
{
  "format": "cassie.catalog.v3",
  "record_version": 1,
  "kind": "type",
  "object_id": "4294967296",
  "revision": "1",
  "database_id": "4294967300",
  "body": {}
}
```

Required keys are exact. Each `kind` has the required/optional body fields below.
Durable scalar codecs serialize in the recorded database encoding context
(client_encoding equal to database encoding), never an unrecorded session encoding.
Unknown format/version/kind/field, duplicate JSON keys, invalid UTF-8, unpaired
surrogates, nonfinite JSON numbers, numeric IDs, missing fields or extra bytes
are rejected before publication. A u64 identity/revision is a decimal string
matching `0|[1-9][0-9]*`, checked against u64; zero is reserved where specified.
Signed values that can exceed the exact JSON integer range are signed decimal
strings with no leading zeros or negative zero. Other bounded integer fields
use JSON integers, never floating point. NULL and absent fields are distinct.
Envelope strings are not Unicode-normalized. SQL identifiers, enum labels,
source definitions and arbitrary SQL text use `SqlBytes = {encoding,
bytes_base64}`: encoding is the exact canonical PostgreSQL encoding name and
base64 is canonical padded RFC4648. The bytes retain PostgreSQL database-context
meaning, including SQL_ASCII bytes that are not UTF-8. Global catalog names
retain original bytes and PostgreSQL caller-context behavior. A decoded display
label is never the authoritative identifier. Plain envelope `name`/`label`
fields below mean SqlBytes, not an unrestricted JSON string.

Canonical bytes use UTF-8, lexicographically sorted object keys, no insignificant
whitespace, escaping quote/backslash as `\"`/`\\`, controls backspace/formfeed/
newline/return/tab as `\b`/`\f`/`\n`/`\r`/`\t`, other U+0000–U+001F controls
as lowercase `\u00xx`, and literal UTF-8 for all other Unicode scalars. Slash
is literal. Bounded integers use shortest decimal with no leading zero or -0. Lists retain their defined order. Hashes use
these bytes without a terminating newline. This is a specified JSON subset,
not a promise that arbitrary `serde_json::to_vec` iteration order is durable.

## Midge keys and publication

In the existing catalog column family, reserve ASCII key prefix
`catalog/v3/`. Object revisions use `catalog/v3/object/` followed by network-endian
u64 database ID, network-endian u64 object ID and network-endian u64 revision. The publication root is
`catalog/v3/root`; its JSON envelope has kind `root`, object_id `"0"`,
revision equal to the committed catalog generation and database_id `"0"`.
Retain an immutable copy of each published root under `catalog/v3/root/` plus
network-endian u64 generation; `catalog/v3/root` contains the identical current
root bytes. Both keys are written atomically. Root records do not include their
own hash in objects. Retain pinned historical roots for transaction/recovery
owners; reclaim only when no live or durable pin can reach them.
Allocator reservations use `catalog/v3/allocator`. No second storage layer is
introduced; older layout readers/writers reject v3 at admission.

The root body is `{generation, objects, live_objects, database_settings, collations}`.
`generation` is a u64 string. `objects` is a list sorted by `(database_id, object_id, revision)`
whose entries are `{database_id, object_id, revision, sha256}`. The SHA256 is lowercase hex
over the canonical referenced record. A root includes every reachable live
object and retained descriptor revision. `live_objects` is a sorted list of
`{database_id, object_id, revision}` references, exactly one current revision
per live object address; each must be in `objects`. Retained historical revisions
do not reappear in namespace lookup or ordinary catalogs merely because their
bytes remain reachable. A dropped object's retained revisions are absent from
live_objects. This explicitly separates current visibility from decode retention. `database_settings` and `collations` are sorted lists of
`{database_id, object_id, revision}` references to database records and exact
collation provider/version records, respectively.
Unknown references, duplicate IDs/revisions, missing objects, hash mismatch or
an invalid dependency graph fail recovery. Do not skip malformed records.

The SQL commit owner writes newly reachable revisions and the root publication
in the same atomic Midge batch as the associated authoritative row/schema
changes. Readers pin the committed root generation and their immutable
descriptors. Publish the in-memory root only after durable commit succeeds.
Rollback/savepoint abort exposes no partial catalog or row mutation. Retain old
descriptors while any authoritative row, active transaction/portal, prepared
transaction or recovery record references them. Derived caches and sidecars are
generation-bound and rebuilt/invalidated; they are not a second authority.
The #833 transaction owner and #904 registry owner implement this publication;
#915 later adds prepared-transaction pin/recovery obligations.

## Object identity and allocator record

Apply the approved named-type u64 allocation range to named catalog objects
that types depend on, including roles, relations, namespaces and routines.
One store-wide monotonic object allocator begins at `2^32`; built-in
objects use their reserved PostgreSQL identities within their catalog/database
context. A record address is `(database_id, object_id, revision)`. Global roles
and the publication/allocator root use database_id 0. Database-local references
inherit the enclosing database unless a database_id is explicit; type and
namespace references cannot cross databases. Role references resolve in global
context. Identical built-in OIDs in different databases are separate catalog
instances; cache keys include database/catalog context. Named object IDs remain
unique throughout the store, including across databases. Type IDs are the IDs of their
type objects. Enum member IDs come from the same allocator. `0` is invalid for
a real object; an ACL grantee ID of `0` means PUBLIC only.

Wire OIDs remain separate u32 catalog identities. The approved high-half named
type OID range is shared by named catalog objects needing SQL OIDs, excluding
all reserved/committed OIDs. Preserve both mappings across rename/restart/import.
No hash-derived OID or OID reuse follows from object deletion. SQL-visible
OID lookups retain the required database/catalog context; OID alone is not a
complete object address. Imported OID/reg*
scalar values retain their actual OIDs because the complete referenced catalog
mapping is restored; never rewrite arbitrary numeric OID values as object IDs.

The allocator body is `{next_object_id, next_wire_oid, exhausted_object_ids,
exhausted_wire_oids, reserved_wire_oid_ranges}`. `next_object_id` is a u64 string;
`next_wire_oid` is a u32 integer; exhaustion fields are booleans. Ranges are
inclusive `[first,last]` u32 pairs, sorted, disjoint and include all reserved
built-in/private identities. Before an identity is exposed, persist an allocator
reservation through the serialized catalog owner. Aborted transactions may
leave gaps. On the maximum identity, mark exhaustion explicitly instead of
wrapping. Import verifies all mappings/high-water marks before publishing a root.
The allocator can advance independently of a rolled-back SQL transaction; the
objects and their root publication remain transactional.

## Shared object and descriptor fields

All named objects have `namespace_id`, `name`, `owner_role_id`, `wire_oid`
(u32 or null when the object has no wire OID), `dependencies` and `acl`.
Dependencies are sorted `{database_id, object_id, revision, kind}` records; `kind` is one of
`normal`, `auto`, `internal`, `extension`, `partition_primary`,
`partition_secondary`, `pinned`. Duplicate dependency records are invalid. A dependency
revision pins a descriptor needed to decode state; ordinary name dependencies
resolve the object's current live revision through the root. ACL entries use
the approved grantor/grantee IDs and valid granted/grant-option masks.

Each type body additionally contains:

- `type_kind`: `scalar`, `enum`, `composite`, `array`, `range`, `multirange`,
  `domain`, `pseudo`, `internal` or the selected `vector` profile.
- `pg_identity`: `{oid, typlen, typbyval, category, preferred, delimiter,
  alignment, storage}` for built-ins, copied from the immutable reference
  registry; null for named types whose identity is described by their kind.
- `codec`: `{tag, version}` or null when no admissible stored non-NULL value
  exists. Tags and versions are those already approved, never guessed from OID.
- `io`: stable routine references for `input`, `output`, `receive`, `send`,
  `typmod_input`, `typmod_output`, `analyze`, `subscript`; absent functions are
  null and retain PostgreSQL's errors. A catalog function existing does not
  establish that it accepts externally constructed values.
- `array_type_id`, `base_collation_id` (object ID strings or null),
  `representation`: the kind-specific fields below.

| Type kind | Required representation fields |
| --- | --- |
| scalar | `semantic_family`, `semantic_contract_version` (u32), `reference_source_sha256` |
| enum | `members`: ordered `{member_id,label}` objects, unique member IDs and labels |
| composite | `fields`: ordered field descriptors, `relation_id` (ID or null) |
| array | `element_type_id`, `element_descriptor_revision` |
| range | `subtype_id`, `subtype_revision`, `collation_id`, `subtype_opclass_id`, `canonical_routine_id`, `subdiff_routine_id`, `multirange_type_id`; optional routines/collation are null |
| multirange | `range_type_id`, `range_descriptor_revision` |
| domain | `base_type_id`, `base_revision`, `base_typmod`, `not_null`, ordered `constraints` |
| pseudo/internal | `semantic_family`, `external_input` (boolean), `stored_nonnull_admission` (boolean) |
| vector | `profile_id`, `profile_version`, `component_type_id`, `dimension_constraint` (positive u32 or null), `extension_id` (ID or null) |

A field descriptor is `{field_id, name, type_id, descriptor_revision, typmod,
collation_id, nullable, dropped}`. `field_id` is positive u32, typmod is i32,
collation is an ID or null, and booleans are strict. Composite field list order
is its positional SQL order; CRB3 directory order is ascending field_id. A
dropped field retains its historical identity/descriptor. Descriptor changes
allocate a new immutable revision. Named enum rename preserves member identity;
addition publishes a new ordered member list. A domain's NULL constraint is
evaluated even though its SQL NULL has no domain payload.

## Other catalog record kinds

The outer version and shared named-object fields apply to these records.
`definition` is `{source_bytes, language, binding_context, dependencies}`;
source_bytes is SqlBytes and language is its stable language object reference.
`binding_context` stores the database/namespace/search_path and referenced
identity map used by the relevant PostgreSQL definition semantics. It never
stores executable Rust/serde AST bytes. Recovery parses definition fragments
under the restored catalog and verifies their references; it does not execute
arbitrary saved DDL. Dynamic or later-bound routine statements are not eagerly frozen/rebound during
recovery. Bind only the fragments PostgreSQL binds at the relevant lifecycle
point; runtime name/search_path/security resolution remains dynamic where
PostgreSQL requires it. Runtime owners specify and qualify those semantics.

| Kind | Required body beyond shared fields |
| --- | --- |
| database | `encoding`, `default_collation_id`, `settings`, `namespace_ids`; database_id equals object_id |
| namespace | `database_id` |
| role | `attributes`, `password_verifier` (secret or null), `memberships`, `settings`; verifiers never enter logs and export only through authorized roles |
| collation | `provider`, `locale`, `deterministic`, `provider_version`, `provider_data_sha256`; version mismatch blocks use/rebuild until owner qualification |
| relation | `relation_kind`, `schema_revision` (u32 CRB3 version), ordered `fields`, `constraints`, `partition`, `inheritance`, `storage_mode`, `derived_objects`; physical Midge CF handles are recreated, not portable IDs |
| sequence | `data_type_id`, signed-string `start`, `increment`, `minimum`, `maximum`, `last_value`, booleans `cycle`, `is_called`, u64-string `cache`, `owned_by` (field reference or null); export includes the actual durable reservation boundary |
| routine | `routine_kind`, `language`, `arguments`, `return_type_id`, `return_revision`, `returns_set`, `volatility`, `strict`, `parallel`, `security_definer`, `definition`, `configuration` |
| constraint | `constraint_kind`, `relation_id`, `field_ids`, `definition`, `deferrable`, `initially_deferred`, `validated`, `referenced_relation_id`, `referenced_field_ids`; irrelevant fields are explicitly null/empty |
| view | `materialized`, `definition`, `row_type_id`, `row_type_revision`, `populated` |
| index | `relation_id`, `access_method`, `keys`, `predicate`, `unique`, `exclusion`, `included_fields`, `options`; imported physical sidecars rebuild |
| sql_object | `object_kind`, `definition`, `signature`, `options`; object_kind identifies aggregate, cast, conversion, operator, operator_family, operator_class, rule, trigger, event_trigger, statistics or extension; its owner supplies the versioned semantic options schema before runtime |

The generic `sql_object` envelope reserves identity and versioning, not a license
for unspecified new persistent options: each owning implementation must first
extend this contract with its exact `object_kind` schema. Those later objects
are final integration obligations, not dependencies on the shared registry's
primitive type codecs. Prepared transactions and large objects also receive
their own #915/#916 exact durable records before their runtime, under this
versioned catalog/root authority.

## Logical export bundle v1

The manifest file is UTF-8 canonical JSON with:

`{format:"cassie.logical-export", version:1, source_layout, source_revision,
source_store_id, catalog_generation, reference_source_sha256, files,
allocator, quiescence, completeness}`.

`source_layout` is the actual source layout; `source_revision` is its immutable
executable revision, never inferred from the destination. `files` is an ordered
list of `{path, kind, byte_length, record_count, sha256}`; paths are relative,
contain no `..`, symlink or absolute path, and are unique. Length/count are u64
strings. Hashes cover exact file bytes. No unlisted file contributes state.
`quiescence` identifies the verified stopped-write boundary/checkpoint;
`completeness` is `complete` only after every authoritative family is included.
An unknown source family, prepared transaction or large-object state without
its qualified adapter makes export incomplete and prevents switching stores.

Each NDJSON line is a canonical JSON object followed by one LF, no blank lines.
Record kinds are:

1. `catalog`: `{kind, record}` using the catalog envelope above, followed by
   exactly one committed root and an allocator reservation record. Emit
   dependencies before dependents where acyclic; mutually referring nominal
   descriptors use a declared identity pass followed by complete definitions.
2. `row`: `{kind, relation_id, schema_revision, row_identity, fields}`.
   `schema_revision` is the u32 CRB3 schema version, distinct from u64 catalog
   object/descriptor revision. `row_identity` preserves the source's logical document/row key; physical CF
   names and sidecars do not define portable identity. `fields` is an ordered
   list of `{field_id,present,datum}`. Absent has present=false, datum=null;
   SQL NULL has present=true and the typed null datum below. Preserve multiplicity.
3. `document`: `{kind, relation_id, row_identity, document_base64}` for the
   exact validated source document bytes when those bytes are authoritative.
   A relation's export adapter specifies whether rows or documents are the
   authority; never import both as independent logical rows.
4. `sequence_state`: `{kind, sequence_id, last_value, is_called,
   reserved_through}` with signed decimal strings for state values. This state
   agrees with the catalog sequence and its actual durable allocation boundary.
5. Reserved versioned kinds `prepared_transaction` and `large_object` require
   #915/#916 adapters and schemas before they can appear in a complete export.

Every datum has exactly `{type_ref, descriptor_revision, typmod, is_null,
codec_tag, codec_version, payload_base64}`. `type_ref` and descriptor revision are
u64 strings, typmod is i32, null is boolean, tag/version are u16 integers.
For SQL NULL, codec_tag, codec_version and payload_base64 are null;
type_ref/descriptor_revision/typmod still preserve its declared identity. For non-NULL, tag/version are
required and payload_base64 uses canonical padded RFC4648 base64 containing
the exact approved durable payload. No JSON number carries an exact SQL value.
Interpretation uses the referenced immutable descriptor, not a type-name guess.

V2 adapters export actual declared/stored semantics. A historical NUMERIC alias
uses its FLOAT8 descriptor and exact IEEE payload; JSONB aliases use their
actual JSON descriptor. Old custom array/vector identities are translated using
their source descriptors and registered destination identity mapping. They do
not silently adopt standard PostgreSQL OIDs or exact numeric/JSONB semantics.
An alias label can be retained as provenance, never used to reinterpret bytes.

## Admission, verification and rollback

Validate hashes/counts/schema/versions/dependencies/identities/typed payloads and
resource bounds before destination publication. Build into a fresh separate
store. `catalog/v3/import` is a global envelope of kind `import_state`, body
`{manifest_sha256, phase}`, phase one of `loading`, `rebuilding`, `verified`,
`complete`. Missing/unknown/non-complete import state blocks client admission
for an import store. Authoritative rows can be loaded in bounded batches while
hidden; the final atomic batch publishes the verified root and phase `complete`.
Failure leaves no committed import root visible to clients. Initial import has
no resume promise; retain a failed destination for inspection or recreate it. Preserve
old IDs and wire OIDs when valid, with an explicit complete mapping for any
approved v2 identity translation. Reject collisions rather than silently rename.

Independent verification compares catalog meaning, exact typed values, NULL and
absent fields, multiplicity, sequences/reservations/allocator state and row keys
against the quiesced source. Rebuild every derived index/column/vector/search
sidecar, then restart and repeat the comparison. Restore routine/security
dependencies and qualify privileges, transaction publication, prepared
transactions and large objects before claiming those families migrated.
Clocks, random data and physical plans use the documented semantic assertions.

Keep the original store/executable unchanged and record their hashes/location.
Switch clients only after verification, with new connections/prepared statements.
Rollback reconnects to the preserved original pair; post-switch writes need a
separately verified reverse-export procedure, not an old writer on v3 bytes.

## Approval and remaining acceptance boundary

Approve this concrete catalog/export envelope, key namespace and identity
extension within the already selected v3 direction. This would authorize the
contract records and their tests; no runtime or migration is implemented by
approval. Each exact per-family semantic/codec/resource record, independent
golden/negative witness and owner dependency remains required. Later SQL-object,
prepared-transaction and large-object schemas stay gated by their named owners.
The complete ordered gate, adversarial probes, exact-head hosted checks and
checked integration still apply. #793 and #849 remain open until their actual
acceptance criteria are satisfied.
