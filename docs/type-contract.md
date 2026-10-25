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
| `name` | 19 | 64 | UTF-8 name identity, limited to 63 bytes; text input truncates on a UTF-8 boundary, binary receive rejects values over 63 bytes |
| `text` | 25 | -1 | UTF-8 |
| `char(n)` / `character(n)` / `bpchar(n)` | 1042 | -1 | `atttypmod` is `n + 4`; unbounded `bpchar` has typmod -1; bare `CHAR` is `char(1)` |
| `varchar(n)` / `character varying(n)` | 1043 | -1 | `atttypmod` is `n + 4` when representable |
| `uuid` | 2950 | 16 | |
| `bytea` | 17 | -1 | Raw bytes; hex text output |
| `date` | 1082 | 4 | |
| `time` | 1083 | 8 | |
| `timestamp` | 1114 | 8 | Without time zone |
| `json` | 114 | -1 | Existing JSON identity |
| `jsonb` | 3802 | -1 | Distinct JSONB identity selected on 2026-10-24; implementation and qualification are in progress |
| `jsonpath` | 4072 | -1 | Expression-only identity; JSONPath columns and arrays are not selected |
| `vector(n)` | `100000 + n` | -1 | Private identity; finite positive dimension |
| `T[]` | PostgreSQL's registered array OID for supported built-in element types | -1 | Standard identity; unsupported/private element families are excluded from pgwire |

### Selected NAME introspection identity

`name` (OID 19, `typlen` 64) is selected for `current_catalog`,
`current_database()`, and `pg_client_encoding()` results; `name[]` (OID 1003)
is selected for `current_schemas(boolean)`. The runtime uses UTF-8 strings and the
existing JSON array carrier; row persistence reuses the existing string and
array tags, so no prior row format changes. Text NAME input clips to the
longest UTF-8 prefix of at most 63 bytes, matching PostgreSQL's `namein`;
binary NAME input rejects 64-byte values, matching `namerecv`. NAME binary
output sends the unpadded text bytes, matching `namesend`.

This selection does not add general `name[]` parameter, constructor, operator,
or assignment qualification. Those remain pending until their own issue tests
exercise those paths.

### Selected JSONB contract (2026-10-24)

JSONB is selected as a distinct SQL type rather than a JSON alias. JSON retains
OID 114; JSONB uses OID 3802 and its registered array identity is OID 3807.
JSONB text output uses PostgreSQL canonical object-key ordering, last-key
precedence for duplicate object keys, and exact decimal document numbers.
Binary JSONB is one version byte (`1`) followed by the UTF-8 JSON document;
malformed documents and unsupported versions must fail deterministically.
Extraction preserves the document input type, while text extraction returns
TEXT. JSONB operators and their inferred parameters retain JSONB identity.

Storage continues to use the existing JSON document row encoding and Midge
storage layer. New JSONB columns persist their distinct schema type. Existing
schema entries serialized as `json` remain JSON: the former alias erased the
original SQL spelling, so it cannot be recovered by an automatic reinterpretation.
No row-layout migration or additional storage abstraction is selected.
This contract is authorized; support remains Experimental until its selected
runtime, wire, restart, NULL and exact-value tests have passed.

The selected inspection increment uses the existing native document identities:
`json_typeof`/`jsonb_typeof` return TEXT, `json_array_length`/`jsonb_array_length`
return INT4, and `jsonb_exists_any`/`jsonb_exists_all` accept JSONB plus TEXT[]
and return BOOLEAN. Each is strict for SQL NULL; document null remains a JSON
value. This bounded #810 increment depends on the implemented finite #809
operators and native type identities. JSONPath is a separate selected #809
increment; stored JSON text preservation, JSON aggregates and broader array
layouts remain separate pending obligations.

The selected construction increment adds `json_build_object`, `json_build_array`
and `to_json` with JSON identity, and their JSONB counterparts with native JSONB
identity. JSON constructor objects retain input key order and duplicate keys;
JSONB construction applies canonical ordering and last-key precedence. Scalar
SQL text becomes a JSON string, document inputs remain documents, and exact
SQL numeric values remain exact. Builders include SQL NULL as document null;
scalar conversion functions preserve SQL NULL. This increment reuses the
existing document and row encodings.

The JSON object constructor overloads `json_object(text[])` and
`json_object(text[], text[])`, plus JSONB counterparts, accept the existing
one-dimensional TEXT[] carrier. The alternating form requires an even number
of elements; parallel key/value arrays require equal lengths. SQL NULL keys or
values are rejected. JSON output preserves constructor order; JSONB canonicalizes
keys. This does not select general SQL array layouts or multidimensional arrays.

The selected SQL/JSON subset includes `JSON(input WITH/WITHOUT UNIQUE KEYS)`,
`JSON_ARRAY(SELECT ... ORDER BY ...)`, and `JSON_EXISTS(document, path ... ON
ERROR)`. JSON input preserves the supplied document text; WITH UNIQUE KEYS
checks duplicate keys recursively and reports SQLSTATE 22030, while WITHOUT
UNIQUE KEYS accepts duplicates. The selected JSON_ARRAY query form preserves
the query's order and uses ABSENT ON NULL behavior. JSON_EXISTS maps structural
JSONPath errors to FALSE, TRUE, SQL NULL for UNKNOWN, or propagates the error
for ERROR ON ERROR; omitted policy defaults to ERROR ON ERROR. These finite
forms do not select general `IS JSON`, `JSON_VALUE`, `JSON_QUERY`, `PASSING`,
or other JSON constructor clauses. Keep those as pending contract obligations
until a read-model use case and finite semantics are selected. Likewise,
`array_to_json`/`row_to_json` remain unselected legacy conversion overloads.

### Selected JSONPath expression identity (2026-10-24)

JSONPath expressions use PostgreSQL OID 4072 and `typlen` -1 for SQL casts,
parameters, operators and selected functions. This is an expression-only
identity: JSONPath table columns and JSONPath arrays are not selected, and no
row-blob or persisted schema representation is introduced. The bounded runtime
surface is recorded in [Feature Support](feature-support.md) and remains
Experimental until its selected parser, accessor, predicate, item-method,
resource and cancellation evidence passes.

The `varchar` identity also accepts `char varying(n)`,
`national character varying(n)` and `nchar varying(n)`. These spellings retain
OID 1043 and the same `n + 4` typmod as `character varying(n)`.

`bpchar` and `pg_catalog.bpchar` without a length use the existing unbounded
character identity (OID 1042, typmod -1). Their `(n)` forms use the same
character-counted padding and `n + 4` typmod as `char(n)`. Bare SQL `CHAR`
remains width one and is distinct from unbounded `bpchar`.

`NULL` uses the unknown result identity (OID 705, `typlen` -2, typmod -1);
SQL NULL on the wire is still a field length of -1. `oid` and the supported
`reg*` aliases use their distinct registered PostgreSQL identities; their
finite behavior is described under [Feature Support](feature-support.md#planning-execution-and-interfaces).

Supported arrays use the PostgreSQL catalog array OID registered for their
element type (for example, `int4[]` uses OID 1007); Cassie legacy array OIDs are
not accepted as wire identities. This identity rule does not define the complete
element-family, dimension, lower-bound or document-null behavior, and scalar
support does not imply matching array support. ARRAY(VECTOR) remains excluded
from pgwire. Array parameter binding remains limited to the forms listed in
[Feature Support](feature-support.md).

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

The finite mutation increment adds `jsonb_set`, `jsonb_set_lax`, and
`jsonb_insert` with native JSONB results and TEXT[] paths. Missing intermediate
containers remain unchanged. `json_strip_nulls` and `jsonb_strip_nulls` retain
their input document identity; `jsonb_pretty` returns TEXT. These operations
reuse existing document encodings and require admitted decoding and output
scratch, cancellation checks, and deterministic path/input errors.

### Selected integer result widths (2026-10-24)

Arithmetic, remainder and bitwise expression results preserve their resolved
INT2/INT4/INT8 identity and binary width. Mixed integer operands use the widest
operand type, while shifts preserve the left operand type. Overflow is checked
at the resolved width. The former projected BIGINT widening is retired; no
column, runtime integer carrier or durable row encoding changes are required.

## Unselected type families

Native MONEY, range/multirange, XML and geometric types remain proposals under
the [Product Roadmap](product-roadmap.md), outside the active type inventory.
Their historical PostgreSQL conformance tests do not select runtime support.
The [2026-10-24 MONEY audit](testing/scope-audit-2026-10-24.md) records removal
of its 30 unselected parity cases and retention of an explicit unsupported-type
boundary test. Exact NUMERIC read-model values and their coverage remain active.
