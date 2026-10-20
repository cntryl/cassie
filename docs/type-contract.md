# Finite SQL Type and Wire Contract

This document inventories Cassie's current SQL type identities, metadata and
storage boundaries. [Feature Support](feature-support.md) owns behavior and
support status; the [Product Roadmap](product-roadmap.md) owns product scope.
PostgreSQL 18.6 is a semantic reference for selected behavior, not a promise of
complete type, cast, array, catalog or server parity.

The runtime has two type lanes: the original scalar/container carriers and
native families represented by `DataType::Extended`. Object-identifier types
have their own typed identity. A declared scalar's presence here does not imply
that every cast, operator, function or corresponding array form is supported.
See [Extended Types](extended-types.md) for family implementation details and
the [historical #752 checkpoint](type-contract-history.md) for superseded
qualification snapshots. Its old pending statements do not describe current
type support.

## Original scalar and container identities

These entries use the existing `DataType` adapters. Character typmods are
reported from the declared column length. Bare `FLOAT` is Cassie's float8
identity; `FLOAT(p)` selects that identity or the separate native REAL family
below according to the declared precision.

| SQL type | OID | `typlen` | Notes |
| --- | ---: | ---: | --- |
| `smallint` | 21 | 2 | Signed 16-bit value |
| `integer` | 23 | 4 | Signed 32-bit value |
| `bigint` | 20 | 8 | Signed 64-bit value |
| `float`, `float(p)` (25 through 53), `float8`, `double precision` | 701 | 8 | IEEE float8; bare `FLOAT` selects float8 |
| `boolean` | 16 | 1 | |
| `text` | 25 | -1 | UTF-8 |
| `char(n)` / `character(n)` | 1042 | -1 | `atttypmod` is `n + 4`; bare `CHAR` is `char(1)` |
| `varchar(n)` / `character varying(n)` | 1043 | -1 | `atttypmod` is `n + 4` when representable |
| `uuid` | 2950 | 16 | |
| `bytea` | 17 | -1 | Raw bytes; hex text output |
| `date` | 1082 | 4 | |
| `time` | 1083 | 8 | |
| `timestamp` | 1114 | 8 | Without time zone |
| `json` | 114 | -1 | `jsonb` is not a distinct supported type |
| `vector(n)` | `100000 + n` | -1 | Private identity; finite positive dimension |
| `T[]` | `34000 + (element identity mod 10000)` | -1 | Private identity; vector elements use a legacy residue and their wire arrays are excluded |

The `varchar` identity also accepts `char varying(n)`,
`national character varying(n)` and `nchar varying(n)`. These spellings retain
OID 1043 and the same `n + 4` typmod as `character varying(n)`.

`NULL` uses the unknown result identity (OID 705, `typlen` -2, typmod -1);
SQL NULL on the wire is still a field length of -1. `oid` and the supported
`reg*` aliases use their distinct registered PostgreSQL identities; their
finite behavior is described under [Feature Support](feature-support.md#planning-execution-and-interfaces).

The array formula is Cassie's existing identity adapter. For VECTOR elements,
the adapter uses the legacy `33000 + n` element residue before applying the
array formula. Neither formula defines the complete element-family, dimension,
lower-bound or document-null behavior, and neither is the PostgreSQL standard
array ABI. In particular, scalar support does not imply matching array support.
ARRAY(VECTOR) remains excluded from pgwire.

## Native extended identities

The following names, OIDs and lengths are registered by `ExtendedType`. A
typmod is shown where a declaration can carry one. Temporal precision is
0 through 6; `numeric(p,s)` retains its declared precision and scale.

| Type family | SQL spelling | OID | `typlen` | Typmod |
| --- | --- | ---: | ---: | --- |
| REAL | `real`, `float4`, `float(p)` (1 through 24) | 700 | 4 | -1 |
| NUMERIC | `numeric`, `decimal` | 1700 | -1 | precision/scale when declared |
| Network | `inet` | 869 | -1 | -1 |
| Network | `cidr` | 650 | -1 | -1 |
| Hardware address | `macaddr` | 829 | 6 | -1 |
| Hardware address | `macaddr8` | 774 | 8 | -1 |
| WAL position | `pg_lsn` | 3220 | 8 | -1 |
| Bit string | `bit(n)` | 1560 | -1 | declared length |
| Bit string | `bit varying(n)` / `varbit` | 1562 | -1 | declared maximum length |
| Temporal | `date` | 1082 | 4 | -1 |
| Temporal | `time(p)` | 1083 | 8 | precision when declared |
| Temporal | `time(p) with time zone` / `timetz` | 1266 | 12 | precision when declared |
| Temporal | `timestamp(p)` | 1114 | 8 | precision when declared |
| Temporal | `timestamp(p) with time zone` / `timestamptz` | 1184 | 8 | precision when declared |
| Temporal | `interval` | 1186 | 16 | field/precision typmod when declared |

The `FLOAT(p)` spelling selects the existing REAL identity for precision 1
through 24 and the existing FLOAT identity for precision 25 through 53. The
precision selects the carrier; it is not retained as a typmod, so both forms
report typmod -1. Values outside that range fail with SQLSTATE 22023. The
`date`, `time` and `timestamp` entries are natively carried during
execution. Their unmodified schema declarations retain the existing base OIDs
and row-blob tags; precision-qualified forms and the timezone-bearing temporal
types use their extended identities. Explicit `WITHOUT TIME ZONE` declarations
retain the `time`/`timestamp` identities, including their precision typmods.
`REAL` is float4, and `NUMERIC` is exact decimal; neither is an alias for
bare Cassie's float8 `FLOAT` carrier.

## Carriers, codecs and persistence

Extended values remain native in the evaluator and use family-specific ordering
keys for comparisons, grouping, joins and indexes. Their canonical PostgreSQL
text output is used at the text-result boundary; supported binary results use
the family's PostgreSQL send format. Input and binary behavior are specific to
each registered family.

For schema fields declared with an extended identity, the row-blob encoder uses
the existing `TYPE_EXTENDED` tag (`0x0e`), followed by a length-prefixed payload
containing the type OID and that type's binary value. The decoder resolves the
OID and invokes the matching family receiver. Existing base scalar, vector and
array row-blob tags remain in place. This inventory does not select a second
storage abstraction, a new migration policy, or a PostgreSQL on-disk format.
Midge remains the only storage layer and owns persistence and recovery mechanics.
`FLOAT(p)` declarations use these existing REAL or FLOAT representations; the
precision does not add a stored tag or a persisted typmod.

NUMERIC carries a base-10 integer mantissa and scale, with native text and
binary codecs, typmod handling, arithmetic and selected aggregate/result-type
rules. Temporal families carry calendar/time values natively and use statement
session settings for timezone, date style, interval style and clock functions.
Network, bit-string, `macaddr`, and `pg_lsn` families have their own values,
binary codecs, ordering and selected operators. The detailed boundaries and
known exclusions live in [Extended Types](extended-types.md) and the canonical
[Feature Support](feature-support.md) matrix.

## Qualification boundary

Type registration and a passing case do not promote a capability. Support
status remains Experimental until the feature owner has the required semantic,
wire, resource, restart and client evidence. PostgreSQL-shaped behavior is
selected per documented use case; unlisted casts, overloads, catalog rows,
array families and extensions remain unsupported or unqualified according to
Feature Support and their live issue criteria.

The former #791–#793 full PostgreSQL parity and CRB3 export/recreate/reimport
program is closed as superseded. Its approval snapshots and staged acceptance
records are retained as history only; they do not authorize a storage migration
or expand this type contract.
