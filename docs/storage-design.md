# Cassie Row Storage Design

Implement the Cassie projection storage layer using a row-oriented storage model optimized for Midge and LSM storage.

## Goals

The design must support:

- compact storage
- schema evolution
- efficient point lookups
- projection rebuilds
- secondary indexes
- full-text indexes
- vector indexes

Cassie is a projection and query engine.

Midge is the persistence engine.

Cassie stores projection rows in Midge.

---

# Storage Model

Cassie V1 uses:

```text
Primary Storage:
  Row blobs

Secondary Storage:
  Index structures
```

Every projection record must be stored as a compact binary row blob.

Example:

```text
r/{projection_id}/{row_id}
    -> RowBlob
```

The row blob is the source of truth for projection data.

Indexes are derived structures.

---

# Row Blob Format

Use a compact self-describing binary format.

Structure:

```text
RowBlob
├─ format_version
├─ schema_version
├─ flags
├─ field_count
└─ fields[]
```

Field layout:

```text
Field
├─ field_id
├─ type_tag
├─ value_length (variable-sized types only)
└─ value_bytes
```

Fields must be stored sorted by field_id.

Never store field names in row blobs.

---

# Field IDs

Schema metadata maps names to integer IDs.

Example:

```text
1 -> tenant_id
2 -> status
3 -> created_at
4 -> body
5 -> embedding
```

Rules:

- field IDs are immutable
- field IDs are never reused
- deleted fields become retired
- schema versions are tracked separately

Example:

```text
field 7 = retired
```

Do not recycle IDs.

---

# Type System

Support:

```text
0x00 null
0x01 bool
0x02 i64
0x03 u64
0x04 f64
0x05 string
0x06 bytes
0x07 json
0x08 vector_f32
0x09 timestamp_ms
```

Strings:

```text
UTF-8
```

Vectors:

```text
dimension
f32 values
```

JSON:

```text
raw JSON bytes
```

or CBOR later.

---

# Encoding Rules

Use:

```text
varint field IDs
varint lengths
fixed-width numeric payloads
```

Store:

```text
field_id
type_tag
value
```

for each field.

Variable-length values must include length.

Fixed-width values must not include length.

---

# Primary Key Layout

Projection row:

```text
r/{projection_id}/{row_id}
```

Metadata:

```text
m/{projection_id}/...
```

Indexes:

```text
i/{projection_id}/{index_id}/{value}/{row_id}
```

Search postings:

```text
s/{projection_id}/{field_id}/{term}/{row_id}
```

Vectors:

```text
v/{projection_id}/{field_id}/{row_id}
```

Use big-endian integer encoding for keys so lexicographic ordering matches numeric ordering.

---

# Secondary Indexes

Cassie stores row blobs by default.

Fields may optionally participate in secondary indexes.

Example:

```sql
CREATE INDEX ON applications (status);
CREATE INDEX ON applications (created_at);
```

Generated keys:

```text
c/applications/status/approved/{row_id}
c/applications/created_at/2026-06-18T00:00:00/{row_id}
```

Secondary indexes are maintained automatically.

---

# Full Text Search

Full-text indexes are separate structures.

Example:

```text
s/{projection_id}/{field_id}/{term}/{row_id}
```

The row blob remains the source of truth.

Search indexes are derived data.

---

# Vector Search

Vector indexes are separate structures.

Example:

```text
v/{projection_id}/{field_id}/{row_id}
```

Store:

- vector bytes
- dimensions
- index metadata

The row blob remains the source of truth.

Vector indexes are derived data.

---

# Design Principles

1. Row blobs are the authoritative projection record.
2. Indexes are derived and rebuildable.
3. Field names never appear in stored rows.
4. Integer field IDs are used everywhere internally.
5. Schema evolution must not require rewriting historical rows.
6. Projection rebuilds must be possible from stored row data.
7. Storage format should remain stable and deterministic.

## Acceptance Criteria

The implementation is complete when:

- rows can be encoded and decoded
- field IDs are catalog-driven
- schema versions are tracked
- rows support sparse fields
- indexes can be rebuilt from row blobs
- search indexes can be rebuilt from row blobs
- vector indexes can be rebuilt from row blobs
- schema evolution works without field ID reuse
- storage remains compact and deterministic