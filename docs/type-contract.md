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
| `vector`, `vector(n)` | Active database's installed vector-extension OID | -1 | One canonical identity; bare typmod -1, fixed typmod is raw dimension (1 through 16,000) |
| `T[]` | PostgreSQL's registered array OID for supported built-in element types | -1 | Standard identity; unsupported/private element families are excluded from pgwire |

### Native NAME and NAME[] values

`name` (OID 19, `typlen` 64) is a native scalar type for explicit casts,
assignment, parameters, and results, as well as `current_catalog`,
`current_user`, `session_user`, `current_role`, `current_database()`, and
`pg_client_encoding()`. `name[]` (OID 1003, element OID 19) supports the
existing one-dimensional array input, assignment, and result paths, as well as
`current_schemas(boolean)`. Text and binary parameter/result formats preserve
these PostgreSQL OIDs. The runtime uses UTF-8 strings and the existing JSON
array carrier; row persistence reuses the existing string and array tags, so
no prior row format changes. NAME[] text input uses PostgreSQL array syntax,
while direct array text results retain Cassie's existing JSON-array spelling;
binary NAME[] values use PostgreSQL array framing. Explicit casts from NAME[]
to text use PostgreSQL array text. Text NAME input clips to the longest UTF-8
prefix of at most 63 bytes, matching PostgreSQL's `namein`; binary NAME input
rejects values longer than 63 bytes, matching `namerecv`. NAME binary output
sends the unpadded text bytes, matching `namesend`. Locale-sensitive PostgreSQL
catalog ordering and wider `name[]` bounds/assignment semantics remain outside
this finite carrier contract.

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
the query's order and uses ABSENT ON NULL behavior. JSON_EXISTS maps JSONPath
evaluation errors to FALSE, TRUE, SQL NULL for UNKNOWN, or propagates the error
for ERROR ON ERROR; omitted policy defaults to FALSE, matching PostgreSQL 18.6.
These finite forms do not select general `IS JSON`, `JSON_VALUE`, `JSON_QUERY`,
`PASSING`, or other JSON constructor clauses. Keep those as pending contract
obligations until a read-model use case and finite semantics are selected.
Likewise, `array_to_json`/`row_to_json` remain unselected legacy conversion
overloads.

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

### Selected PostgreSQL array shape contract (2026-10-26)

The user selected PostgreSQL array dimensions and lower bounds for #807. Array
values must retain row-major elements, every dimension length, and each lower
bound across SQL execution, pgwire codecs, and persisted rows. An empty array
has zero dimensions; a SQL NULL array, an empty array, and NULL elements remain
distinct. PostgreSQL's rectangularity and subscript semantics apply. The
0.1.0 CRB1 row encoding stores dimensions, lower bounds, and row-major elements
under `cassie-midge-layout-v1-0.1.0`. Existing pre-release data must be
recreated, with no migration utility or legacy row reader. Runtime work follows
the documented #796 and #762 prerequisites in #807.

### Character array element conversion (2026-11-04)

The existing #796/#807 character-array obligation selects explicit casts to
`CHAR(n)[]`, `VARCHAR(n)[]` and `TEXT[]` over currently selected scalar array
families, using the corresponding scalar conversion policy. This adds no
element family, OID, public value carrier or persisted representation. Preserve
row-major values, every dimension and lower bound, SQL NULL arrays, empty arrays
and NULL elements. Character array OIDs remain 1014/1015; an array descriptor
inherits its element typmod (`n + 4` for these bounded character types).
Explicit casts, ARRAY constructors and typed subquery/subscript expressions
retain their modifier. Ordinary array function, aggregate and concatenation
results have typmod -1; argument modifiers do not become result modifiers.

For already-admitted COALESCE array operands, preserve the element modifier only
when every operand has the same resolved array type and modifier. An untyped
NULL operand removes that modifier; a NULL explicitly cast to the same array
type retains it. This descriptor rule does not add common-type conversions or
ARRAY/CHAR operands to the separate finite NULLIF/GREATEST/LEAST contract.

Explicit casts truncate at character boundaries and CHAR pads with ASCII
spaces. A CHAR element converted to TEXT or VARCHAR drops insignificant
trailing ASCII spaces. Unknown array literals are parsed without the target
length constraint before explicit element conversion. Input/assignment length
validation remains distinct: non-space overflow fails `22001`; only excess
ASCII spaces may be discarded. Implicit assignment must not acquire explicit
truncation. Native query values materialize CHAR padding under query admission;
the existing canonical stored CHAR spelling and array row encoding remain.

This law is source-backed by PostgreSQL18.6 `varchar.c` explicit/input paths and
a finite 21-query oracle batch, including Unicode, bounds, NULLs, input errors,
native scalar correspondence and array descriptor typmods. Its implementation and
resource/lifecycle regressions are consolidated in the [query/array closeout](query-array-closeout-2026-11-05.md),
which records integrated qualification. This finite evidence does not establish
general array-cast or assignment parity.

PostgreSQL 18.6 text-array probes return `{01:02:03.1,NULL}` for TIME
and `{"2026-10-05 01:00:00",NULL}` for a TIMESTAMP element supplied
with `+01:00`. Current array normalization and formatting use the internal
fixed-width temporal carrier, including offset conversion, so these wire
values are covered by the retained #807 oracle fixtures and the consolidated
normalization/formatting implementation. The closeout record owns their fresh
execution status.

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


## Selected v0.2 named types and vector identity (2026-11-04)

The user approved this contract direction on 2026-11-04. It is selected
implementation scope, not implemented or qualified support. Existing rejection
and compatibility boundaries remain until the owning acceptance cases pass.

### #828

Add CREATE TYPE name AS (...), relation row types, ROW construction/casts/field access,
typed JSON recordset outputs (implemented by the JSON recordset increment), and CREATE/ALTER/DROP DOMAIN over already supported
scalar/container families. Allocate stable database-scoped type/array OIDs from Cassie's
object allocator rather than copying arbitrary PostgreSQL user OIDs. Preserve field
order, per-field logical identity and dropped-field positions; distinguish SQL NULL
record from a non-NULL record whose fields are all NULL. Anonymous RECORD is transient
and cannot be a stored column. Domains retain named declared identity, base typmods and
CHECK/NOT NULL validation including NULL; common-type/result flattening follows the
selected PostgreSQL oracle. Select rename, attribute add/drop with versioned
descriptors, and domain constraint add/drop only after validation of existing data;
unsupported destructive base-type changes reject before mutation.

Introduce explicit public named-type/record value carriers and immutable versioned
descriptors. Reserve CRB1 additive tags 0x0F for composite and 0x10 for domain
(currently unused). Each new payload begins with version u8=1, type OID u32 and
descriptor revision u64; composite then has checked field count u32 and recursively
framed nullable values, domain one framed base value. New integer fields use big-endian
encoding; existing tags/bytes do not change. Domain NULL uses the outer NULL marker and
cannot wrap a NULL base value to bypass constraints. Existing CRB1 scalar bytes remain
unchanged within the current format. New tagged payloads are validated by the current reader; no pre-release marker transition or migration is required. Retain descriptors needed by current rows, including dropped fields. Text/binary RECORD uses PostgreSQL
field count/OID/length framing; named composites use allocated OIDs, transient RECORD
uses 2249/contextual typmod. Domain output uses base codecs where SQL resolution
flattens it. Catalog/schema/constraint/row changes publish in one family and survive
restart/image restore; failed validation publishes nothing.

#### #828 increment 1 status (composite types and ROW)

Increment 1 implements `CREATE TYPE [schema.]name AS (field type, ...)`,
`DROP TYPE [IF EXISTS] name [RESTRICT]` (`2BP01` on dependents, `42710`
duplicate, `42704` missing, `3F000` missing schema), composite-typed table
columns, `ROW(...)` and implicit `(a, b)` constructors, `ROW(...)::t` and
`'(x,1)'::t` casts, `(expr).field` selection, PostgreSQL record text
input/output, binary RECORD output, the CRB1 tag 0x0F payload (version u8=1,
type OID u32, descriptor revision u64, field count u32, recursively framed
nullable values, big-endian; tag 0x10 carries domains, see the domain status below) and the
`pg_type`/`pg_class`/`pg_attribute`/`regtype` rows. Type, array-type and
`relkind = 'c'` relation OIDs come from `Midge::allocate_object_id` and are
durable. The code and its regressions are authored; they have not been
compiled or executed (assembler-owned gate). Pending or excluded:

- named enums are no longer excluded: they follow the v0.4.0 contract below (#827,
  authored, pending the assembler gate). `ALTER TYPE`, relation row types,
  `(expr).*` and domains are described under increment 2 below; the JSON
  recordset functions and `JSON_TABLE` are implemented and described in
  `docs/feature-support.md`;
- arrays of composite types (`name[]` columns and casts) are rejected with
  `0A000` although the `_name` array type row and OID exist;
- composite columns cannot carry `DEFAULT`, `PRIMARY KEY`, `UNIQUE` or an
  index; `CREATE TABLE AS` with a RECORD column is not yet rejected;
- binary parameter input of composite types, `COPY` of composite columns and
  `DROP TYPE ... CASCADE` with dependents are not implemented;
- record ordering uses an order-preserving key that is exact for equality and
  for text, integer, float and boolean fields; fields of other families order
  by their typed key bytes;
- Describe of a statement parsed without a session resolves composite names against the
  default search path; routine (`CREATE PROCEDURE`) bodies parse without the session type
  scope, so a composite type name inside a procedure body fails with `42704`;
- one simple-query string cannot `CREATE TYPE` and use the type in a later
  statement of the same string (all statements are parsed first);
- `DROP TYPE` dependents cover composite fields and table columns only, not
  views, materialized views, projections or function signatures;
- concurrent `CREATE TYPE` of one name is not one atomic step (check,
  identity allocation and publication are separate), like extensions;
- `TypedValue::send_binary` for a record returns empty bytes on an encode
  error; the wire path (`codecs/record.rs`) propagates errors, so this is
  reachable only through `COPY BINARY` and similar direct callers;
- a prepared statement holds the descriptor from Parse time; after `DROP TYPE`
  plus `CREATE TYPE` of the same name it fails closed with `42846`;
- anonymous `ROW(...)` field types are best effort (integers report
  `bigint`, so a binary RECORD field OID is 20 where PostgreSQL sends 23);
- btree indexes and `UNIQUE` on record columns are `0A000` although
  PostgreSQL supports them;
- database image restore fails closed: restoring an image that contains
  composite types returns `0A000` until the image rewrite re-keys the
  `composite-type` family and re-allocates type identities;
- `DROP DATABASE` discards the database's composite types, and
  `DROP SCHEMA ... CASCADE` is rejected with `2BP01` while an object outside
  the schema uses one of its types. This is a deliberate stricter difference:
  PostgreSQL would cascade-drop the dependent column instead;
- polymorphic (`anyelement`) user-defined functions called with a record
  argument are rejected with `42883` in this increment, although PostgreSQL
  accepts them;
- comparing a record column with a text column or untyped literal returns
  `0A000` ("input of anonymous composite types is not implemented"), where
  PostgreSQL returns `42883` for the column case;
- a composite type owns a `pg_class` row, so it shares the relation namespace with
  tables, views, sequences and indexes (`42P07` for `CREATE TABLE/VIEW/SEQUENCE/INDEX`
  over a composite type name; `CREATE TABLE IF NOT EXISTS` over a type name skips, as
  in PostgreSQL, but `CREATE SEQUENCE/INDEX IF NOT EXISTS` over a type name still
  raise `42P07` where PostgreSQL skips). `CREATE TYPE` over any occupied relation or
  type name is `42710`. A domain has no `pg_class` row and lives in the type namespace
  only; see the domain status below for how it collides.

#### #828 increment 2 status (ALTER TYPE, relation row types, cleanups)

The code and its regressions are authored; they have not been compiled or executed
(assembler-owned gate). Selected contract, verified against PostgreSQL 18.6:

- `ALTER TYPE name RENAME TO | SET SCHEMA | OWNER TO` and
  `ALTER TYPE name RENAME ATTRIBUTE a TO b | ADD ATTRIBUTE a type | DROP ATTRIBUTE
  [IF EXISTS] a | ALTER ATTRIBUTE a [SET DATA] TYPE t`, each optionally followed by
  `CASCADE`/`RESTRICT` (both behave alike: Cassie has no typed tables). One action per
  statement. A table name is `42809` ("is not a composite type"), a missing type `42704`,
  a duplicate attribute `42701`, a missing attribute `42703`, a rename onto an existing
  relation `42P07` (`42710` when the occupant is a domain), a move onto an existing
  table, view, composite type or domain `42710` (`42P07` for a sequence or index), a missing schema `3F000`, a
  missing role `42704`, a self-containing type `42P16`.
- Versioned descriptors: a descriptor carries `revision` and `dropped_positions`. The
  physical slot list only grows. `ADD ATTRIBUTE` appends a slot, `DROP ATTRIBUTE`
  records the slot as dropped (its position is retained, a re-added attribute gets a
  fresh slot), `RENAME ATTRIBUTE` and type renames keep slots. A stored value is a prefix
  of the current layout, so rows written under any earlier revision decode with added
  attributes NULL and dropped attributes hidden. Storage JSON and the CRB1 0x0F payload
  carry one entry per physical slot; the payload format is unchanged.
- `ALTER ATTRIBUTE ... TYPE` would have to rewrite stored values, so it is `0A000`
  ("cannot alter type X because column t.c uses it") while any table column holds the
  type directly or through another composite type, as in PostgreSQL. It runs, with no
  data conversion, on an unused type.
- Every change re-embeds the new descriptor in other composite types, table columns
  (schema, row schema and projection `schema_version`) and view column types. A Midge
  transaction spans one column family and each Cassie database is one family, so the type
  and every dependent in the type's own database commit in ONE transaction
  (`Midge::publish_composite_group`); the in-memory catalog changes only after commit.
  Residual exception: a dependent in a different database is a second transaction,
  published after the type. The type is always persisted first, so an interruption can
  only leave a dependent on an older revision (never newer than the type), and the
  failure path and hydration (`Catalog::reconcile_embedded_composites`) roll such copies
  forward to the persisted type. Materialized-view and projection storage schemas are
  collection schemas like tables, so a composite column in one is covered by the same
  stored-use guard and re-embedding; this is not yet separately exercised. A table's derived row type is re-embedded by a second transaction after
  the `ALTER TABLE` commits; hydration and the failure path reconcile table row types
  too (the derived descriptor is the authority), so an interruption between the two heals
  at the next start. Routine
  signatures are not re-embedded. `ALTER ATTRIBUTE ... TYPE` reports a missing attribute
  (`42703`) before the stored-use `0A000`.
- Relation row types: every table owns a composite type named like the table. Its
  descriptor is derived from the table's current columns. Its `pg_type.oid` and array OID
  are allocated from the same Midge object-id allocator as every other catalog object when
  the table is created, persisted on the collection metadata (`row_type_object_id`,
  `row_array_object_id`) and therefore stable across renames and restarts, always below
  the `2^31` user-OID limit and resolvable through `regtype` (`reltype::regtype`). The
  descriptor revision is the table's persisted projection `schema_version`, which only
  grows (ADD, DROP and RENAME COLUMN each bump it). `pg_class.reltype`, `pg_type` (`typtype = 'c'`, `typrelid` = table) and the `_name`
  array row are published. A table name works as a column type, a cast target
  (`ROW(...)::t`, `'(1,x)'::t`) and a PREPARE parameter type. For a single-table `SELECT`,
  a bare table name or alias is the whole row (`SELECT t FROM t`), `(t).col` selects a
  field, `ROW(t.*)` expands to the columns and `(expr).*` expands a composite column, table
  row or cast in the SELECT list (`0A000` for any other operand). Whole-row forms are not
  rewritten inside subqueries, CTEs, DML or views. A CTE named like the table or alias
  shadows it (no whole-row rewrite). A bare table name that is not a real column of that
  table, used in a join, set operation, `GROUP BY`, `HAVING` or `DISTINCT ON`, is `0A000`
  (PostgreSQL accepts these; Cassie reports the limit instead of a missing column).
- Table DDL keeps the row type in sync: `ADD COLUMN`, `RENAME COLUMN`, `RENAME TO` and
  `SET SCHEMA` re-embed the derived descriptor; stored values read the added column as
  NULL. `DROP TABLE` is `2BP01` (`0A000` with `CASCADE`) while another column or composite
  type holds its row type. `ALTER TABLE ... DROP COLUMN` is `0A000` while the row type is
  stored in another column; PostgreSQL permits it, so this is a deliberate stricter
  difference (Cassie keeps no dropped-column history for table row types).
  `ALTER TABLE ... ALTER COLUMN ... TYPE` is `0A000` ("cannot alter table "t" because
  column "u.x" uses its row type") while the row type is stored, as in PostgreSQL.
- Known limitations: in an unscoped instance a domain label (`regtype`) does not see an earlier-schema table row type that shadows the domain, so it prints `posint` where PostgreSQL prints `public.posint` (scoped instances qualify correctly); views, sequences and indexes have `reltype = 0` (PostgreSQL gives
  views a row type); `CREATE TABLE ... OF type` typed tables do not exist; renaming a
  table does not change its row-type revision; a row type embedded in another table's
  column is re-embedded only by the table DDL paths above.
- Cleanups: `TypedValue::kind()` returns `Option<ExtendedType>` (`None` for a record)
  instead of a sentinel; a stored record that contradicts its column descriptor is
  `XX001` at row decode (and in the typed scan paths) instead of reading as NULL; the
  session type scope covers views, `PREPARE`/`EXECUTE`, `COPY` and the `CREATE TABLE AS`
  reparse; a repeatable-read session resolves type names against its snapshot catalog;
  binary result formats admit a record only for columns flagged `ColumnMeta::is_record`
  (named composites and `ROW(...)`), replacing the OID-range heuristic.

#### #828 increment 2 status (domain types)

Authored, not yet executed (compilation is deferred to the assembled batch):

- `CREATE DOMAIN name [AS] base [DEFAULT expr] [NOT NULL | NULL] [CONSTRAINT n] CHECK (...)`
  over scalar and array base types; `ALTER DOMAIN` SET/DROP DEFAULT, SET/DROP NOT NULL,
  ADD CONSTRAINT CHECK [NOT VALID], DROP CONSTRAINT [IF EXISTS], VALIDATE CONSTRAINT,
  RENAME TO and RENAME CONSTRAINT; `DROP DOMAIN [IF EXISTS] [RESTRICT]` with `2BP01`
  detail for dependent table columns. Domain DDL shares the `CREATE TYPE` statement,
  transaction and catalog-publication paths, so it is transactional.
- Domain-typed table columns and `x::d` casts. Enforcement runs on INSERT, UPDATE,
  COPY and casts: `23514` for a failed CHECK (constraint name in the message) and
  `23502` for NULL against NOT NULL. CHECK is satisfied by TRUE or NULL. A domain
  DEFAULT applies only when the column has no default of its own.
- `VALUE` in a CHECK is the checked value. CHECK bodies are stored as SQL text and
  evaluated by the scalar evaluator; unnamed checks are named `{domain}_check`,
  `{domain}_check1` and so on.
- `ALTER DOMAIN ADD CHECK` and `SET NOT NULL` validate existing rows and publish
  nothing on failure, and validation sees the session's own staged rows. `NOT VALID`
  checks are enforced on INSERT/COPY and on every newly assigned value of an UPDATE or
  `ON CONFLICT DO UPDATE`; only an unchanged legacy value is exempt until validated.
- `ALTER TABLE ADD COLUMN` of a domain column enforces the domain NOT NULL and CHECK
  constraints on a populated table and backfills the domain DEFAULT. A domain shares the
  type namespace with composite types, table, view and materialized-view row types and
  other domains, and always collides with them as `42710`. Sequence and index names live
  only in the relation namespace and do not collide with a domain (PostgreSQL 18.6),
  except that `CREATE SEQUENCE` over a domain name is `42710`. `ON CONFLICT DO NOTHING` still raises `23514` / `23502`
  or `DO UPDATE` for a violating proposed value. `pg_attribute.atttypmod` of a domain
  column is `-1`. A domain named like a table, view, sequence or composite type is
  `42710` for `CREATE TABLE [IF NOT EXISTS]`, `CREATE VIEW`, `CREATE MATERIALIZED VIEW`,
  `CREATE SEQUENCE` and `CREATE TYPE`, and `CREATE DOMAIN` collides with tables, views,
  composite types and other domains; a domain may share a sequence or index name.
  `ALTER TABLE/MATERIALIZED VIEW ... RENAME TO` or `ALTER TABLE ... SET SCHEMA` onto a domain
  name is `42710` (`ALTER VIEW` is not implemented and stays `0A000`), onto a composite type name `42P07`; `ALTER SEQUENCE` and `ALTER INDEX`
  renames onto a domain name are accepted, as in PostgreSQL.
- `ALTER TYPE <domain> RENAME TO | SET SCHEMA` and `DROP TYPE <domain>` act on the domain
  (`ALTER TYPE <domain> ADD ATTRIBUTE ...` is `42P01`, `OWNER TO` is `0A000`);
  `ALTER TYPE <composite> RENAME TO <domain>` is `42710`.
- A table's row type keeps the domain of a domain column: `'(-1)'::t`, `ROW(-1)::t` and
  `jsonb_populate_record(NULL::t, ...)` raise `23514`/`23502` as in PostgreSQL.
- Whole-row reads (`SELECT t FROM t`, `t::text`) never revalidate stored values; a row
  type's domain fields resolve the live domain descriptor by OID when the statement is
  parsed, so `ALTER DOMAIN` is seen by later casts. `(row).field` infers the base type.
  A column whose type is another table's row type stores that row type with its domain
  fields flattened and enforces the live domains of the row type by OID on INSERT and
  UPDATE, one level deep: a row-type field that is itself another table's row type is not descended into (PostgreSQL enforces at any depth), and `UPDATE t SET r.a = ...` field assignment is not supported (the target is rejected as an unknown column). Known divergences: binary `record_send`/`record_recv` carry
  the domain OID per field for casts and user composites (as PostgreSQL), but a
  whole-row value (`SELECT t FROM t`) is built from an unconstrained descriptor and
  sends the base OID; a `JSON_TABLE` `EXISTS PATH` column of a domain type casts the
  boolean to the domain even when the path is not found (PostgreSQL raises `42804`);
  bind-time re-parse of view bodies uses the live catalog, not the open transaction's
  overlay or REPEATABLE READ snapshot, so a domain created in the same open
  transaction as `CREATE VIEW` is not seen by the bind step; a null-safe expression
  `DEFAULT` that casts to a domain (for example `DEFAULT lower('A')::d`) is folded
  against an empty catalog and fails with `42704` (plain literal casts such as
  `DEFAULT 5::d` are stripped and accepted).
- View, materialized view and materialized projection bodies, and their dependency
  and rename re-checks, resolve domain, composite and row-type names against the
  catalog and search path of the statement.
- Domain names are accepted as column types of `json[b]_to_record[set]` `AS t(a d)`
  lists and `JSON_TABLE` columns: the value converts to the base type, then the domain is
  enforced (`23514`; a `JSON_TABLE` column under its default `ON ERROR` yields NULL).
- Known divergences: `UPDATE t SET q = c` where the stored value already equals the
  constant `c` and violates a `NOT VALID` check is accepted (PostgreSQL re-coerces the
  constant and rejects); a column with its own volatile DEFAULT falls back to the domain
  default when added to a populated table; adding a column whose domain default is
  `gen_random_uuid()` to a populated table is `0A000` (a single value would not be
  distinct per row).
- Flattening: operators and functions resolve through the base type, and
  RowDescription, `atttypmod` and parameter types report the base OID. A table column
  keeps its domain identity separately (`FieldMeta.domain`), so `ALTER DOMAIN` never
  rewrites rows; enforcement looks the live descriptor up by OID.
- Catalog: `pg_type` rows with `typtype = 'd'`, `typbasetype`, `typtypmod`,
  `typnotnull` and a `_name` array row; `pg_attribute.atttypid` of a domain column is
  the domain OID; `'d'::regtype` resolves and prints. `typdefault` and
  `pg_constraint` rows for domain checks are not published.
- CRB1 tag `0x10`: version u8 = 1, type OID u32, descriptor revision u64, one framed
  base value. Decoding yields the base value; a NULL base inside a payload is corrupt.

Rejected with `0A000` (documented limitations, not regressions):

- domains over composite types (including a table's row type), over other domains, and
  over arrays of those;
- arrays of domains (`d[]`) and composite attributes of domain type; a domain name is
  only recognised in column definitions, casts, `json[b]_to_record[set]` definition lists
  and `JSON_TABLE` columns, so function signatures, sequences and `CREATE TYPE`
  attributes report the type as missing (`42704`);
- `DEFAULT <expr>::domain` literal folding resolves no catalog names;
- `COLLATE` on a domain, `ALTER DOMAIN OWNER TO` and `ALTER DOMAIN ... SET SCHEMA` (use
  `ALTER TYPE <domain> SET SCHEMA`);
- `DROP DOMAIN ... CASCADE` when dependents exist (use `RESTRICT` semantics);
- restoring a database image that contains domain types fails closed;
- prepared statements keep the descriptor seen at Parse time.

#### Residual divergences from PostgreSQL 18.6 (#828, #818; accepted, documented)

- `(expr).*` on an expression that is not a composite is `0A000`; PostgreSQL gives `42809`.
- Binary record output re-parses the rendered text of each field, so `timestamptz` and
  domain fields are approximate, and whole-row values send base type OIDs.
- Views have `pg_class.reltype = 0`; PostgreSQL gives views a non-zero row type.
- A plain `CREATE FUNCTION` does not detect a procedure of the same name; PostgreSQL gives
  `42723` because functions and procedures share `pg_proc`.
- Routine owner checks are not enforced.
- `CREATE OR REPLACE FUNCTION/PROCEDURE` with a different argument list is `0A000`, because
  Cassie has no routine overloads.

### #843, #844

Select only pgcrypto 1.4 and vector 0.8.7 as built-in SQL shims. Root installs into an
existing selected schema; support IF NOT EXISTS, selected schema relocation,
same-version UPDATE no-op, DROP and transactional dependency rules; reject unknown
versions/extensions and native library loading. digest accepts TEXT/BYTEA plus SHA256
only, is strict for SQL NULL, returns BYTEA 17 and resolves only while pgcrypto is
installed. For vector, select one allocated per-database canonical type OID with
dimension in typmod, plus bare vector typmod -1 with dimensions inferred from each
finite f32 value. Keep vector arrays outside this issue and report unsupported metadata
truthfully. Select <-> L2, <#> negative inner product and <=> cosine plus matching
documented opclasses/ANN options, pinned to pgvector 0.8.7 oracle behavior.

Persist extension identity/version/schema/owner and registered routine/type/operator
identities as versioned owning-family records. Installation/removal and dependent
catalog changes are atomic and recoverable. Keep existing vector row representation and
declared dimensions; canonical SQL descriptors use the installed type OID and typmod
instead of private 100000+n. Binary vector framing remains dimension int16, reserved
zero int16, big-endian f32 components with finite validation. This is an explicit public
wire identity change: invalidate prepared descriptors and require reconnect/reprepare
when the profile activates; do not silently reinterpret previously supplied private
OIDs. Reserve legacy OID ranges from dynamic allocation. Existing Rust DataType OID
helpers need a reviewed contextual resolver contract. DROP RESTRICT protects live
dependent columns/indexes; CASCADE behavior must remove actual dependents atomically.
Restore retains extension IDs and rebuilds derived ANN state from authoritative vectors.

#### #844 vector type identity and resolution

Jev selected the explicit active-database type-context direction (probability 0.48,
confidence 0.31). This is a weak design lead, not correctness evidence. Source review
confirms that vector extension identity is owned by per-database `ExtensionMeta`, while
`DataType` is shared by parsing, logical planning, storage schemas, and code paths with
no database context. Therefore keep `DataType::Vector` logical and dimension-bearing,
and resolve its PostgreSQL identity only through the active database catalog. For bare
vector, Jev selected `DataType::Vector(Option<usize>)` with `None` representing the
unconstrained logical type and `Some(n)` representing `vector(n)` (probability 0.84,
confidence 0.79). This is a design lead; source review must verify every conversion and
wire/catalog boundary. Its request and response are in the ignored
`target/v020-evidence/844-vector-contract/bare-vector-type-model-*` files. The
active-database resolver request and response are retained in the same evidence
directory.

The vector extension record owns one allocated vector type object identity per
database. Its catalog OID is resolved from that persisted identity; it is not derived
from the dimension. A declared `vector(n)` uses the same OID as bare `vector`, with
`atttypmod = n`; bare `vector` uses `atttypmod = -1`. The declared typmod is the raw
dimension, not the string-type `+4` adjustment. The pgvector 0.8.7 source confirms that
`vector_typmod_in` returns the supplied dimension and its receive/input paths compare
that typmod directly with the value dimension ([`vector.c`](https://github.com/pgvector/pgvector/blob/v0.8.7/src/vector.c#L2296-L2307),
[`typmod input`](https://github.com/pgvector/pgvector/blob/v0.8.7/src/vector.c#L2717-L2757)).
The installed base type uses `typcategory = 'U'`: pgvector's `CREATE TYPE vector`
does not set `CATEGORY`, and PostgreSQL 18 defaults base types to the user-defined
category ([pgvector 0.8.7 type definition](https://github.com/pgvector/pgvector/blob/v0.8.7/sql/vector.sql#L2194-L2208),
[PostgreSQL 18 `DefineType`](https://github.com/postgres/postgres/blob/REL_18_STABLE/src/backend/commands/typecmds.c#L2429-L2464),
[type-category constants](https://github.com/postgres/postgres/blob/REL_18_STABLE/src/include/catalog/pg_type.h#L1466-L1468)).
`V` is reserved for PostgreSQL bit-string types.
Its selected dimension limit is 1 through 16,000
([`VECTOR_MAX_DIM`](https://github.com/pgvector/pgvector/blob/v0.8.7/src/vector.h#L255),
[`CheckDim`](https://github.com/pgvector/pgvector/blob/v0.8.7/src/vector.c#L2312-L2334)).

All SQL type binding, `pg_type`/`pg_attribute` identities, prepared parameter
admission, pgwire text/binary codecs, and result descriptions that handle vector must
use the active database resolver. Context-free `DataType::type_oid()` and
`vector_dimensions_for_oid()` must not identify or accept vector protocol/catalog
types. Binary input gets its dimension from the frame, validates the 1..=16,000 bound,
reserved zero word, exact frame length and finite f32 values, then checks a declared
typmod when present. Catalog metadata exposes one installed vector type identity per
database; vector arrays remain unsupported. Reserve the old dimension-derived OID
range 100001..=132767 from dynamic object-OID allocation, and never accept or publish
those IDs as vector types.

Type identity resolution and prepared result descriptors use the current database
catalog version. If extension activation changes a prepared result's resolved type
identity, preserve the existing cached-result-type rejection and require the client to
reprepare or reconnect; do not silently reinterpret a previously supplied private OID.
Extension DROP continues to reject live column/index dependents under RESTRICT and
removes actual dependents atomically under CASCADE.

Jev selected storing this identity as `ExtensionMeta.vector_type_object_id: Option<u64>`
(confidence 1.0). Allocate the vector type object ID from the same Midge catalog object-ID
counter as other normal objects; resolve its PostgreSQL OID through the existing
`normal_oid` mapping. The vector extension record sets this field, while the pgcrypto
record leaves it empty. The changed durable extension record uses format version 2;
pre-release extension metadata is recreated, with no compatibility reader or migration.
The request and response are retained in the ignored
`target/v020-evidence/844-vector-contract/vector-type-identity-storage-*` files.

Jev selected a finite typed `VectorCatalogIdentitySet` owned by `ExtensionMeta` for the
pgvector access-method, operator, operator-family, and operator-class identities
(confidence 1.0). Allocate each published object identity from the common Midge counter
and resolve it through `normal_oid`; persist the complete set on the vector extension
record and leave it empty for pgcrypto. `pg_amop` rows are identified by their operator
family, operand types, strategy, and purpose and do not receive a separate object OID.
The method/operator/family/class rows are derived from this persisted set and disappear
with the extension record. This remains the current unreleased format version 2, with no
compatibility reader or migration. The request and response are retained in the ignored
`target/v020-evidence/844-vector-contract/vector-catalog-identity-storage-*` files.

Jev selected the additive pgvector method profile, finite method/operator-class
catalog identities, and pgvector's 2,000-dimension ANN limit (probabilities 1.0,
0.99, and 1.0). These are scoped design selections; the pinned PostgreSQL oracle and
Cassie implementation/tests remain the correctness evidence. The typed request and
response are retained in the ignored `target/v020-evidence/844-vector-contract/`
directory.

Extension gate (user-approved, kept): `VECTOR(n)` and bare `vector` columns, `::vector`
casts, and vector wire/parameter OIDs require `CREATE EXTENSION vector` in the
database; otherwise Cassie returns SQLSTATE `42704` (`type "vector" does not exist`).
Only `root` may install extensions. There are no private or legacy vector OIDs: vector
identity is the installed per-database type OID
(`ExtensionMeta::vector_type_object_id`). REST collection creation that declares a
vector field installs the extension through the administrative path when it is missing
(`should_install_vector_extension_when_rest_collection_declares_vector_field`), so REST
clients need no SQL step.

Keep Cassie's existing `USING vector` form and add pgvector's `USING hnsw` and
`USING ivfflat` forms. Each added method accepts exactly `vector_l2_ops`,
`vector_ip_ops`, and `vector_cosine_ops`, mapping respectively to `<->` L2 distance,
`<#>` negative inner product, and `<=>` cosine distance. HNSW accepts `m` and
`ef_construction` index options and the `hnsw.ef_search` query option; IVFFlat accepts
the `lists` index option and `ivfflat.probes` query option. Keep Cassie's existing
`USING vector` options and semantics intact. Other vector metrics, vector storage
types, access methods, operator classes, and ANN options are outside this profile.
Only IVFFlat defaults to `vector_l2_ops`; HNSW requires an explicit operator class.
The catalog marks only the IVFFlat L2 class as default, matching the pinned 0.8.7
SQL profile. Operator-class names resolve in the installed vector extension schema:
an explicit schema must name that namespace, while an unqualified name requires the
extension schema to be visible through the effective SQL `search_path` (including
PostgreSQL's implicit `pg_catalog` entry).
Cassie's legacy `USING vector` index method keeps its existing retrieval behavior,
but it is not a PostgreSQL access method and remains absent from `pg_class`,
`pg_index`, `pg_indexes`, and `regclass` name resolution. Those catalogs publish
vector indexes only for explicit HNSW/IVFFlat methods with installed extension
identities, preventing `relam = 0` rows without changing legacy index execution.
`DROP EXTENSION vector` with dependent vector-typed columns, ANN indexes, or regular
stored views using vector casts/operators fails with SQLSTATE `2BP01` and
preserves all objects. `CASCADE` removes those regular views atomically with the
extension. If vector columns or ANN indexes also depend on the extension, `CASCADE`
is rejected with `0A000` until atomic same-database-family cleanup is implemented;
remove those dependents first. Vector-dependent materialized views are also listed
as dependents, but `CASCADE` rejects with `0A000` until extension, output rows, indexes,
and materialized-view metadata can be deleted atomically; remove those views first.

The view dependency collector covers vector casts/operators across projection and
predicate expressions, query sources, nested subqueries, CTEs, and set branches; view
output fields typed as vector are dependencies even when no operator is used. Extension
dependency IDs also flow through referenced regular views, so CASCADE removes the
dependent view chain. A Jev
source review selected no concrete gap (probability 0.81), while its broader missed-AST
path estimate was 0.40 and triggered a focused probe. A transitive dependency review
raised the missed-path probability to 0.71 and selected an additional AST probe; the
authored CTE-based parent/child view regression now checks that dependency IDs flow
through a text-only child. The authored
`should_track_vector_dependencies_in_nested_select_shapes` regression covers CTE,
scalar-subquery, VALUES-source, and set-right forms; a follow-up rated a remaining
source gap at 0.24 and selected that probe (0.91). It remains unexecuted until the
consolidated bundle gate. Jev selected an explicit `0A000` boundary for vector-dependent
materialized views (probability 0.70) because their output state is not covered by the
regular-view deletion transaction; a focused authored regression verifies rejection
preserves the materialized view. Separate authored local-restart regressions check
stable vector catalog identities and restored regular-view dependency rejection/cascade;
all remain unexecuted. A later combined review rated a remaining stored-view gap at
0.68 and a catalog/object-identity gap at 0.73; Jev selected a mixed-dependency
CASCADE preservation probe (probability 0.82, confidence 0.76). The authored
`should_preserve_vector_dependents_when_extension_drop_is_rejected` regression now
combines a regular vector-dependent view with a vector column and ANN index, checks that
the rejected CASCADE preserves all three dependency classes, and verifies the retained
ANN index OID, `relam`, `pg_am`, and `regclass` identities agree. It remains unexecuted.
The selected dependency collector follows vector use visible in a
stored-view query and extension IDs inherited from referenced stored views; it does not
inspect SQL user-defined function bodies. Jev selected this explicit 0.2.0 boundary
(0.92 probability, 0.89 confidence) after reviewing a text-returning function whose body
uses vector. The request and response are retained under ignored
`target/v020-evidence/844-vector-schema/jev-user-function-vector-dependency-*` files.

Exact vector operations continue to accept dimensions 1 through 16,000. HNSW and
IVFFlat admission is limited to dimensions 1 through 2,000, matching the pgvector
0.8.7 ANN support table ([HNSW](https://github.com/pgvector/pgvector/blob/v0.8.7/README.md#hnsw),
[IVFFlat](https://github.com/pgvector/pgvector/blob/v0.8.7/README.md#ivfflat)); this
does not lower the vector type's input or storage limit.

When vector is installed, publish only the finite truthful vector method profile in
`pg_am`, `pg_operator`, `pg_opfamily`, `pg_amop`, and `pg_opclass`, alongside the one
canonical vector `pg_type` row. Keep those allocated identities stable for that
database and owned by the vector extension record, and remove their dependent catalog
rows atomically on extension DROP. The selected HNSW/IVFFlat operator classes and their
ordering operators are defined by the pinned [pgvector v0.8.7 SQL profile](https://github.com/pgvector/pgvector/blob/v0.8.7/sql/vector.sql#L2641-L2716).
Do not publish generic PostgreSQL method or operator-class rows that Cassie does not
execute. Cassie's existing vector index implementation remains the execution engine;
these SQL methods and catalog identities are aliases to its supported HNSW and
IVFFlat paths, not claims of the native pgvector ABI.

#### #844 v0.2.0 operator semantics and deferred surface

Qualified v0.2.0 behavior, each pinned by a case in
[`pgvector_semantics.rs`](../tests/compatibility/lifecycle/indexes/pgvector_semantics.rs)
(PostgreSQL behavior for the same SQL-visible surface: pgvector 0.8.7):

- `<->` is L2 distance, `<=>` is cosine distance and `<#>` is the **negative** inner
  product (`'[2,2,2]' <#> '[3,1,0]'` is `-8`).
- The distance operators bind tighter than comparison operators:
  `embedding <-> $1 < 0.5` is `(embedding <-> $1) < 0.5`.
- SQL NULL on either operand yields NULL for every operator.
- Operands with different dimensions raise SQLSTATE `22000` with the pgvector message
  `different vector dimensions N and M`.
- `$1::vector` resolves to the database's installed vector type OID at Parse; binary
  parameters use the dimension/reserved/f32 framing above. An untyped parameter (OID 0)
  that is an operand of `<->`, `<#>` or `<=>` is inferred as that vector type in
  ORDER BY, WHERE and projection positions, and Describe reports the vector OID.
  Parameter OIDs carry no dimension; dimension mismatches surface at execution.
- Inner-product and cosine HNSW/IVFFlat indexes are created, cataloged and maintained,
  and the operator ordering returns exactly the unindexed top-k. The SQL operator ANN
  execution path itself serves L2 only; `<#>` and `<=>` ordering is answered by the
  exact scan. Serving those metrics from the ANN graph/lists through the SQL operators
  is a performance extension, not a v0.2.0 correctness requirement.

#844 acceptance evidence map: type identity/OIDs and `CREATE EXTENSION` gating
(`vector_type_identity`, `vector_extension.rs`); operators, precedence, NULL,
dimension error, parameters and deferred rejection (`pgvector_semantics.rs`); operator
classes, HNSW/IVFFlat catalogs, dependencies, dimension limit and L2 ANN-versus-exact
correspondence (`pgvector_profile.rs`); inner-product and cosine top-k equality with
indexes present (`pgvector_semantics.rs`).

Defects found while qualifying and fixed: `$1::vector` was unresolvable at Parse
(`42P18`); `a <-> b < n` parsed as `a <-> (b < n)`; undefined functions returned
`42601` instead of `42883`; non-text values cast to `vector` returned `22000`
instead of `42846`; the dimension-mismatch message did not match pgvector.

Deferred (not v0.2.0, no partial support): the pgvector SQL functions `l2_distance`,
`inner_product`, `vector_dims` and `vector_norm`, and casts from `real[]` (or any other
array or non-text value) to `vector`. Cassie's own `vector_distance`, `cosine_distance`
and `dot_product` functions are unrelated existing surface. The deferred names are
rejected deterministically: calling them returns `42883` (undefined function), and
casting an array, integer or other non-text value to `vector` returns `42846`.
Text-to-vector casts (`'[1,2,3]'::vector`) are the only supported conversion; casts
from `vector` to `real[]` are likewise unsupported (they fail input validation).

These durable features use the current format and must pass current-build restart and recovery checks. No pre-release compatibility transition is required; see [Release Baseline](release-baseline.md#pre-release-format-and-api-policy-2026-11-10).
Named ENUM #827 follows the [selected v0.4.0 contract](#selected-v040-named-enum-contract-2026-11-13); historical full-parity/CRB3 proposals
remain superseded. Codec goldens, checked framing and version admission precede
new row-value admission. Existing tags and scalar bytes retain their meaning.

## Selected v0.4.0 named enum contract (2026-11-13)

Owner: [#827](https://github.com/cntryl/cassie/issues/827). Baseline: `main` @ `af66698d6`.
Semantic oracle: PostgreSQL 18.6 (local throwaway cluster, 2026-11-13). Contract
options were selected with Jev 1.14.0. The request and response are kept under the
worktree's ignored `target/jev/enum_contract_*` files. This section is the design
authority for #827. It supersedes the earlier sketch in
[PostgreSQL 18 type families](postgresql18-type-families.md#named-enums--827) wherever
the two differ.

### Scope

Supported:

- `CREATE TYPE name AS ENUM ('label', ...)`, including an empty label list.
- `ALTER TYPE name ADD VALUE [IF NOT EXISTS] 'label' [BEFORE | AFTER 'label']`.
- `ALTER TYPE name RENAME VALUE 'old' TO 'new'`.
- `ALTER TYPE name RENAME TO`, `SET SCHEMA` and `OWNER TO`.
- `DROP TYPE [IF EXISTS] name [, ...] [RESTRICT | CASCADE]`.
- Enum columns, including `NOT NULL` and defaults.
- Typed literals and casts (`'x'::t`, `CAST('x' AS t)`).
- Implicit coercion of an unknown-type literal to the enum type in comparisons,
  `IN`, `= ANY`, `INSERT`/`UPDATE` targets and `VALUES`.
- Enum ↔ `text`/`varchar` casts and parameters typed by the enum OID.
- Comparison operators `= <> < <= > >=`, `ORDER BY`, `GROUP BY`, `DISTINCT`, `min`/`max`.
- `enum_first`, `enum_last` and `enum_range` (one and two argument forms).
- `t[]` columns, literals and parameters.
- `pg_type`/`pg_enum` rows and `regtype` resolution.

Not part of v0.4.0, rejected deterministically with `0A000`:

- `DROP TYPE ... CASCADE` when dependents exist. This matches composite and domain types.
- Enums as composite fields or as domain base types.
- Enum values in COPY.
- Range types over enums.

PostgreSQL never allows enum labels to be dropped, and Cassie does not either.

### Logical identity

- An enum type is a database-scoped, schema-qualified named type. It shares the
  relation/type namespace with tables, composites and domains. Name resolution uses
  search order exactly like composite and domain types.
- The type OID and array OID are allocated from Cassie's persistent object allocator
  (`allocate_object_id`, then `normal_oid`).
- Each label (member) gets an immutable `u64` member ID from the same allocator.
  `pg_enum.oid` reports that ID.
- The allocator commits independently of the DDL overlay. An ID allocated in a
  transaction or savepoint that rolls back is burned and never reused. Committed IDs
  are never reused.
- `RENAME VALUE` changes only the label of an existing member. Its member ID and its
  ordering are unchanged.
- `ADD VALUE` creates a new member. It never changes the ID or relative order of an
  existing member.

### Ordering

- Each member has an immutable `f32` sort order, stored in the descriptor.
- `CREATE` assigns the sort orders 1, 2, …, n.
- A trailing `ADD VALUE` uses the current maximum plus 1.
- `BEFORE`/`AFTER` uses the midpoint of the neighbouring sort orders, or the neighbour
  minus 1 at the head. This matches PostgreSQL's observed `pg_enum` (`1`, `1.5`, `2`, `3`).
- When a midpoint is not representable strictly between its neighbours, all sort
  orders of the type are renumbered 1…n in the same publication. The relative order of
  the members does not change. Member IDs, stored values and index keys are unaffected.
- Comparison, `ORDER BY`, `min`/`max` and `enum_range` follow sort order. `NULL` follows
  the normal `NULLS LAST`/`FIRST` rules.
- Comparing values of two different enum types, or an enum with a `text`-typed operand,
  is `42883`.

### Durable representation

- Rows store the member ID, never the label text or the sort order.
- The CRB1 additive tag is `0x11` (enum). Its payload is:
  - `version u8 = 1`
  - type OID `u32`
  - member ID `u64`

  Integers are big-endian. Existing tags and bytes keep their meaning.
- `NULL` uses the outer NULL marker.
- The stored JSON form, which is also the scalar-index key, is
  `{"$cassie_enum_v1": <member_id>}`.
- Every storage carrier uses this one form: rows, column batches, materialized views,
  scalar indexes and transactional overlays. Label text appears only at the SQL, wire
  and JSON-function boundary. A decoder rejects an unknown member ID for the column's
  type as corruption (`XX001`).
- Enum arrays use the existing array carrier, with each element in the scalar form above.
- Scalar indexes on enum columns are keyed by member ID. The planner uses them only for
  equality and `IN`/`= ANY`. Range predicates and `ORDER BY` on enum columns use runtime
  comparison by sort order. `ADD VALUE` and `RENAME VALUE` never touch indexes or rows.

### Catalog, persistence and lifecycle

- The descriptor is `EnumTypeMeta`. It is persisted as `serde_json` under the key family
  `b"enum-type"` in the type's database and reloaded at hydration. Its fields are:
  - `format_version`
  - `database`
  - canonical name
  - type OID and array OID
  - owner OID
  - `revision`
  - members: an ordered list of `{member_id, label, sort_order}`
- Each alteration bumps `revision` and publishes the descriptor atomically with the
  catalog change.
- Enum DDL participates in explicit-transaction DDL through the catalog and storage
  overlays, like composite types. The `enum_types` map is published through the catalog
  snapshot delta.
- `ADD VALUE` inside an explicit transaction follows PostgreSQL 18:
  - If the type existed before the transaction, using the new label in that transaction
    raises `55P04`.
  - If the type was created in the same transaction, the new label is usable immediately.
- Dependencies are computed by scanning, like composite types: table columns and
  arrays, defaults, and view or materialized-view outputs.
  - `DROP TYPE ... RESTRICT` with dependents raises `2BP01` with PostgreSQL's
    `column c of table t` detail.
  - `DROP SCHEMA`/`DROP DATABASE` follow the composite type rules.
- Labels are 1–63 bytes. A longer label raises `42602`.

### SQL and wire behaviour

- `pg_type` reports `typlen 4`, `typbyval t`, `typtype 'e'`, `typcategory 'E'`,
  `typelem 0` and a non-zero `typarray`. Each type has an `_name` array row. Cassie's
  `pg_type` has no I/O-function columns (`typinput`, `typoutput`, `typreceive`,
  `typsend`) for any type, and this milestone does not add them.
- `pg_enum` lists `oid`, `enumtypid`, `enumsortorder` and `enumlabel`. It moves out of
  the empty-registry list.
- `RowDescription` reports the enum's own type OID (array: its array OID), not `text`.
- Text output and binary `enum_send` both send the UTF-8 label.
- Binary `enum_recv` and text input parse a label of the current revision.
- A parameter whose described or declared OID is the enum (or its array) parses label
  text.
- `information_schema.columns` reports `USER-DEFINED` with the `udt_name` set.

| Input | SQLSTATE |
|---|---|
| Unknown label in input or cast | `22P02` |
| Duplicate label in `CREATE` | `23505` |
| `ADD VALUE` duplicate (`IF NOT EXISTS` emits a NOTICE instead) | `42710` |
| `RENAME VALUE` to an existing label | `42710` |
| Unknown `BEFORE`/`AFTER` or `RENAME VALUE` source label | `22023` |
| Label longer than 63 bytes | `42602` |
| Cast between enum and an integer type | `42846` |
| Comparison with `text` or another enum type | `42883` |
| `enum_first`/`enum_last` on an empty enum | `55000` |
| Use of an uncommitted added label | `55P04` |
| Drop with dependents | `2BP01` |

### Evidence required for closure

- Codec golden tests for tag `0x11` and for an enum array.
- Restart round-trip after `ADD VALUE BEFORE`, `RENAME VALUE` and `RENAME TO`. This
  enables the existing authored tests in
  `tests/compatibility/lifecycle/types/enum_*.rs`.
- Index equality probes before and after `RENAME VALUE`.
- Transaction rollback, savepoint and `55P04` cases.
- pgwire text and binary result and parameter cases.
- The Prisma enum fixture accepted.
- Every SQLSTATE in the table above checked against PostgreSQL 18.6.
