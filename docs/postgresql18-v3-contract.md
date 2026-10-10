# Selected v3 type identity and durable-value contract

Status: common direction explicitly approved by the user on 2026-10-10 for
[#793](https://github.com/cntryl/cassie/issues/793); not implemented or qualified.
The exact approval snapshot has SHA256
`26c43b1718f94160be6031d9bfc32ae301575df63ab71367498394e77f04ecc6` and is
[retained verbatim](../tests/corpus/postgresql18/v3-approved-proposal.txt).
This readable record restates the approved direction; the framing interpretation
below makes the existing conventions explicit.
The [family records](postgresql18-type-families.md) and
[resolution/resource rules](postgresql18-type-resolution-resources.md) clarify
admission under this direction. The exact
[catalog/export schemas](postgresql18-catalog-export-proposal.md) remain a
separate pending proposal, not an implied persistent selection.
Source baseline: Cassie `769d4db8`, `src/midge/row_blob.rs`, its `encoding.rs`,
`src/types/schema.rs` and the original `docs/extended-type-decisions.md`.

## Selected direction

Retain the direct Midge row/schema organization. Use a versioned typed row format, canonical PostgreSQL 18.6 binary scalar payloads where they preserve identity, and dedicated stable-identity payloads for named/container types. Keep durable type identity independent of PostgreSQL wire OID. Do not persist a Rust enum discriminant, serde enum layout, or a JSON number as an exact SQL numeric carrier.

## Exact common row framing

All integers use network byte order. The existing header/directory structure remains recognizable:

| Bytes | Meaning |
| --- | --- |
| 4 bytes `CRB3`, u8 `3` | Row magic and version; reject every other pair |
| u32 schema_version, u32 max_field_id, u32 bitmap_len | Checked schema/version and bitmap framing |
| bitmap_len presence bytes, bitmap_len SQL-null bytes | Existing one-based field-bit convention; absent and SQL NULL remain distinct |
| u32 directory_count | Number of directory entries |
| Repeated u32 field_id, u16 codec_tag, u16 codec_version, u32 offset, u32 length | Fields in ascending field_id; unique IDs, bounded complete partition of payload |
| Remaining payload | Concatenated non-NULL field values; SQL NULL has no payload |

The retained schema version identifies immutable field descriptors: stable type ID, type descriptor revision, typmod, collation identity and nullability. A schema change retains old descriptors until no authoritative row references them. Unknown schema/type/tag/version, duplicate IDs, invalid bitmaps, overlapping/gapped ranges, trailing bytes and malformed values fail before publication. Counters never wrap. Existing v2 rows are never parsed as v3.

## Identity and codec assignments

Built-in durable type IDs equal their PostgreSQL 18.6 OIDs, widened to u64. Named type IDs allocate monotonically from `2^32` through `u64::MAX` under transactional catalog ownership, never reuse a committed identity and fail closed on exhaustion. Their PostgreSQL wire OIDs allocate independently from `2^31` through `u32::MAX`, excluding every reserved/committed identity; OIDs survive restart and rename. Arrays and multiranges have their own type identities. Failed allocations may leave gaps. New catalogs preserve allocation high-water marks on rollback/restart/import.

Reserve codec tags (each starts at codec_version 1):

1. Built-in scalar with a PostgreSQL binary codec: validated canonical PostgreSQL 18.6 binary send payload, with its exact family grammar frozen in the owning eight-field record. This includes exact NUMERIC, FLOAT4/FLOAT8, temporal, text/bytea, JSON/JSONB, bit, network, XML, geometry, money and identifier families. Never use PostgreSQL internal heap/varlena bytes. Scalar OIDs are read from the descriptor, not embedded as a substitute for durable identity.
2. Named enum: u64 immutable member ID, resolved against the retained type descriptor. Member rename changes the catalog label, never stored membership.
3. Named composite: u32 field count, then each immutable descriptor field in order as a recursively framed nullable value. Null record and a non-NULL record whose fields are all NULL remain distinct.
4. Array: u32 dimension count; each dimension i32 length and i32 lower bound; then row-major nullable elements. Element descriptor/type identity comes from the array descriptor. SQL NULL and JSON document null remain distinct.
5. Range: u8 PostgreSQL range flags, followed by each present finite bound as a framed subtype value. Validate canonicalization and flags against the registered range descriptor.
6. Multirange: u32 range count followed by length-framed canonical ranges, normalized according to the registered subtype/range laws.
7. Domain: recursively framed base datum, validated using the domain descriptor/constraints on admission. Its durable identity remains the domain, including NULL/domain constraint behavior. SQL NULL uses only the enclosing NULL marker; a non-NULL domain payload containing a NULL base datum is noncanonical and rejected.
8. Selected vector profile: u32 dimension count followed by exact finite network-endian f32 components. Extension/wire identity remains the #844 record; the durable format does not infer a pgvector version promise.

9. ACL item (OID 1033, whose PostgreSQL catalog explicitly has no binary send/receive): u64 stable grantor role ID, u64 stable grantee role ID (0 denotes PUBLIC), u32 granted privilege mask, u32 grant-option mask. Role rename changes labels, not the stored reference; grant-option bits must be a subset of the granted mask and both must use valid PostgreSQL 18.6 privilege bits. Wire binary requests retain the PostgreSQL error behavior; this private durable payload does not invent a public binary ABI. Internal `gtsvector` also has no binary codec or usable public input constructor: retain its catalog/error behavior, without admitting invented external values or treating PostgreSQL internal index bytes as a Cassie row value.

A recursively framed nullable value is u8 state (0 SQL NULL, 1 non-NULL), followed for state 1 by u16 codec_tag, u16 codec_version, u32 payload_length and payload. The enclosing immutable descriptor supplies the exact type. Composite field count, array dimensions/product, range bounds and domain nesting must match it. No other state/tag/version is admitted. PostgreSQL pseudo-types not legal in stored columns receive no durable payload; their expression/call identities still belong to the shared registry.

Scalar tag 1 is admitted only where a real codec and lossless family record exist. Pseudo-types/internal carriers without an admissible stored SQL value receive truthful catalog/error behavior and no invented row codec. This common framing does not choose sort/index keys. Equality/order/hash and collation use the registered PostgreSQL semantic laws, never raw wire-byte ordering. Every derived index/column/vector/search sidecar is rebuilt from authoritative imported typed values and qualified separately.

## Upgrade and logical export

Choose a versioned lossless NDJSON export bundle with a SHA-256 manifest, explicit record kinds and dependency order. A datum is `{type_ref, descriptor_revision, typmod, is_null, codec_tag, codec_version, payload_base64}`; no exact SQL number is represented as a JSON number. Catalog records describe object/type/enum-member identity, dependencies, relation field descriptors, privileges, sequences and their allocation high-water marks. User documents remain documents, with explicit SQL-versus-document NULL. Existing historical NUMERIC/JSONB aliases export using their actual v2 FLOAT/JSON interpretation. Export schemas and completeness checks must be specified in #793/#846 before the exporter is implemented.

Use a fresh separate store, reject unsupported records before publication, verify exact values/multiplicity/schema/dependencies/sequences and restart against the quiesced original, then switch clients with fresh sessions/prepared statements. Keep the original store and executable for rollback. No in-place conversion, old/new mixed writer or unverified reverse export is selected.

## Acceptance still required

This approval selects the common v3 byte/identity direction. It does not close #793, approve unspecified family semantics/resources, change runtime or imply upgrade readiness. Family records must include exact scalar encodings/typmods/coercions, resource limits and cancellation, stable catalog serialization/recovery, malformed-input negative controls, pinned PostgreSQL text/binary and exact durable golden witnesses, cross-path/restart evidence and export/reimport verification. The full ordered repository gate, adversarial review and checked integration remain mandatory.

## Framing interpretation

The directory contains present non-NULL fields only; absent and SQL NULL fields
have no directory entries or payload. For field ID n>=1, bit index is n-1,
least-significant bit first within each byte. `bitmap_len=ceil(max_field_id/8)`;
unused bits are zero and each NULL bit has a presence bit. Empty non-NULL values
retain a directory entry with length zero. The directory is sorted by field ID;
offsets partition the payload without gaps, overlaps or trailing bytes.
Descriptor/row schema versions remain u32; catalog descriptor revisions are a
separate identity. See the resource rules and literal acceptance vectors.

PostgreSQL binary scalar send payloads are serialized in the recorded database
encoding context, with client_encoding equal to database encoding. Public wire
output still follows the client's encoding. Legal wire normalization is distinct
from canonical durable admission; special catalog int2vector/oidvector send
payloads remain tag1 and retain their legal empty values even where PostgreSQL's
public receive routine rejects that same empty frame. No private durable rule
may silently narrow legal public SQL input.
