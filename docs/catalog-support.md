# Catalog Support

Cassie's virtual catalogs describe Cassie objects for documented read-model workflows. They are
not PostgreSQL system catalogs, and PostgreSQL-internal catalog parity is not claimed. Only the two
views below are Stable. Every other `information_schema` or `pg_catalog` view remains Experimental
and may add or change columns before 1.0.

Several dated checkpoints below preserve issue references from the former
PostgreSQL 18 full-parity program. #791–#793 are closed as superseded; those
references are historical provenance, not current dependencies. Current product
scope comes from the [roadmap](product-roadmap.md), and live issue status belongs
to GitHub.

Consumers must use an explicit `ORDER BY` when multi-row order matters. Cassie currently emits
these virtual rows deterministically, but implicit relation order is not part of SQL semantics or
the compatibility contract. An unavailable virtual view is an undefined relation and pgwire returns
SQLSTATE `42P01`; Cassie does not synthesize undocumented PostgreSQL-internal rows.

## Stable named-client subset

### `information_schema.tables`

| Column | Cassie contract |
| --- | --- |
| `table_schema` | Schema containing the Cassie table or view. |
| `table_name` | Local table or view name in the connected database. |
| `table_type` | `BASE TABLE` for Cassie tables or `VIEW` for Cassie views. |

Rows are scoped to the connected database and reflect create, rename, drop, and restart hydration.
The sqlx 0.8.3 and Diesel 2.2.6 retained probes use this view for named table discovery.

### `information_schema.columns`

| Column | Cassie contract |
| --- | --- |
| `table_schema` | Schema containing the Cassie table or view. |
| `table_name` | Local table or view name in the connected database. |
| `column_name` | Cassie field name. |
| `data_type` | Cassie's PostgreSQL-facing type name. |
| `ordinal_position` | One-based field position. |
| `is_nullable` | `NO` for a non-null field constraint; otherwise `YES`. |
| `column_default` | Supported default expression, or null. |
| `udt_name` | PostgreSQL-facing underlying type name. |
| `character_maximum_length` | Declared character bound where applicable, or null. |
| `numeric_precision` | Declared numeric precision where applicable, or null. |
| `numeric_scale` | Declared numeric scale where applicable, or null. |
| `datetime_precision` | Declared timestamp precision where applicable, or null. |

Rows are scoped to the connected database and reflect create, rename, drop, and restart hydration.
Repository ORM metadata tests cover the complete supported column shape. Named-client promotion is
limited to the read-model discovery performed by sqlx 0.8.3 and Diesel 2.2.6; it does not certify
migration generation or PostgreSQL catalog introspection beyond these columns.

## Retained client evidence

| Client | Source revision | Workflow run | Certified catalog workflow |
| --- | --- | --- | --- |
| sqlx 0.8.3 | `926401240249bcda832b75f8c2cb9a8c7e1e28fe` | 32503127504 | Connect and discover a named table through `information_schema.tables` using a bound parameter. |
| Diesel 2.2.6 | `01ca3c3393e64379cdd9a692ac1aca0c6b7d5a0b` | 32508304197 | Connect and discover a named table through `information_schema.tables` using a bound parameter. |

Both manifests record `status=passed`, `deployment_profile=loopback-memory`, and
`protocol=pgwire-extended-query`. They certify only the pinned client, source revision, and query
shape. They do not certify client migrations, GUI navigation, extensions, undocumented columns,
OID compatibility, or hosted PostgreSQL behavior.

## Lifecycle and failure evidence

- `catalog_introspection` in `tests/catalog_security.rs` covers table discovery, complete column
  order after restart, and rename/drop visibility after restart.
- `catalog_orm_metadata` in `tests/catalog_security.rs` covers the complete supported column shape,
  defaults, nullability, and type metadata.
- `compatibility_matrix` in `tests/compatibility.rs` verifies pgwire SQLSTATE `42P01` and relation
  metadata for an unavailable relation. The same error contract applies to unavailable virtual
  catalogs.
- Broader native pgAdmin-shaped queries remain deterministic repository tests, but pgAdmin and
  DBeaver certification remains separately tracked and is not promoted by this contract.

## Selected catalog read-model checkpoint (2026-10-16)

The b0d2b65e existing catalog/codec foundation supplies the prerequisite for a
read-model-only #817 batch. PostgreSQL 18.6 column-name shapes are available for absent
foreign/replication/security-label/transform, membership/default-ACL, large-object,
enum/partition/trigger/policy/event-trigger and parameter-ACL registries. Empty
rows describe the absence of these objects in Cassie's current catalog; they do
not implement their DDL or advertise the corresponding capability. Keep native
catalog/domain result OIDs and opaque type codecs outside this selected #817
read-model batch; #793 was a superseded parent specification. Existing logical
carriers remain explicit experimental descriptors. Do not
weaken retained tests that require native type identities.

Table/view/sequence discovery reads real current-database catalog objects.
Create/drop/restart, database isolation, index presence and supported serial
allocation probes qualify the selected rows. Ordinary column identity and
generation fields report NO/NULL/NEVER. Empty stand-ins for dependency,
rewrite, statistics, operator-family or built-in text-search catalogs are
omitted: implemented objects require their actual semantic rows. The finite
`pg_depend` sequence-ownership rows and `pg_shdepend` database CONNECT grant
rows documented below are derived only from existing catalog metadata.
Compiled plans rebuild at epoch 14. No persistent metadata or layout changes
are selected by this batch.

Sequence probes use the implemented SERIAL/BIGSERIAL column defaults and
omitted-field INSERT path. Standalone nextval() and CREATE SEQUENCE AS are not
implemented; exploratory native probes confirm their required state/bounds law,
but those grammar/function prerequisites remain with #841. Keep the existing
advanced sequence qualification tests red until that owner implements them.
The read model reports the existing stored declared type; it does not change
legacy CREATE SEQUENCE's integer default or claim PostgreSQL's bigint default.
Unknown owners remain NULL, and view definitions expose stored SQL rather than
claiming PostgreSQL's pretty-print rendering.

This checkpoint does not promote a catalog to Stable or complete #817. Native
catalog-domain OIDs/codecs, ownership, pretty-printed definitions, system-object
rows, OID/reg* lifecycle and wider DDL remain unqualified; current issue criteria
define any active follow-up.
Native column names and selected values are independent pinned observations;
logical-carrier descriptors do not establish PostgreSQL descriptor ABI parity.
Executed counts and remaining failures are recorded in
[validation](testing/validation.md).

A focused native probe shows PostgreSQL's hasindexes flag remains true after
DROP INDEX until maintenance clears it. The current read model reports current
index presence and clears it immediately; this historical-flag compatibility
cell remains open under #817 and requires its persistent-state/lifecycle design.

### Sequence ownership dependencies (2026-10-24)

`pg_depend` exposes its seven selected PostgreSQL 18 columns and real ownership
rows for sequences with a valid `OWNED BY` table column. SERIAL and explicit
sequence ownership use dependency type `a`; identity sequences use `i`. The
referenced table OID and one-based column position come from the current
catalog schema, and sequence/table OIDs use their existing `pg_class` identities.
Unowned sequences produce no dependency row. Other dependency families remain
unimplemented and are not represented by empty or fabricated rows. This read
model slice changes no persistent metadata or public Rust API.
The selected native index flag probes cover pre/post-create states, not this
unimplemented post-drop law.

### Shared database grant dependencies (2026-10-24)

`pg_shdepend` exposes its seven selected PostgreSQL 18 columns and rows for
database CONNECT grants present in Cassie's role metadata. These shared rows
use `dbid = 0`, `classid = 1262` (`pg_database`), `objid` for the granted
database, `objsubid = 0`, `refclassid = 1260` (`pg_authid`), the granted role's
OID as `refobjid`, and `deptype = 'a'` for the ACL dependency. Grant and revoke
updates are reflected in subsequent catalog reads across database sessions.
Relation-owner dependencies and other shared dependency families remain
unimplemented and are not fabricated. This slice changes no persistent
metadata format or public Rust API.

## Experimental scalar catalog helpers (2026-10-17)

`pg_size_bytes(text)` parses signed decimal/exponent inputs and the native
bytes/B/kB/MB/GB/TB/PB units, using exact decimal multiplication and halfway
rounding away from zero. Numeric input bounds precede unit validation; the
final result must fit bigint. NULL remains NULL. `pg_size_pretty(bigint)` uses
PostgreSQL's unit thresholds and symmetric rounding, including both bigint
limits. This helper does not implement a `pg_size_pretty(numeric)` overload. It
does not
approximate it through floating point or implement physical size APIs.

`pg_char_to_encoding` resolves the pinned PostgreSQL 18.6 alias table for
literal, stored TEXT and explicitly typed TEXT parameters. It applies native
NAME's 63-byte UTF-8 truncation before lookup. Lookup does not enable another
client/database encoding. Native NAME parameter identity (OID 19) remains
unsupported in this checkpoint: an untyped direct parameter rejects with 0A000, rather than
advertising TEXT as its native inferred identity. An explicit `$1::TEXT` uses
OID 25 and the selected name coercion. Non-ASCII lookup observations are pinned
to the recorded Darwin oracle; broader locale/encoding qualification remains
under #821. This helper retains stable rather than immutable classification.

`pg_settings_get_flags(text)` returns PostgreSQL flag metadata for the finite
known name `work_mem` (`{EXPLAIN}`) and SQL NULL for unknown names. This does not
add `work_mem` to Cassie's supported session-setting registry or make it
available through `current_setting`/`SET`.

The additive `SizeInputError`/`CassieError::SizeInput` API carries typed invalid
size, numeric syntax, numeric range and bigint range failures. Size constants
report failures during Bind planning, preserving statements across Sync for
reuse. Parse rejects ambiguous untyped `pg_size_pretty` and invalid selected
signatures. Compiled plans rebuild at epoch 15. No persistent format, layout or
stored type changes. Full resource/cancellation qualification remains #849;
the selected cancellation witness checks already-cancelled execution and does
not establish interruption inside a long parser loop.

Current executed checks and remaining failures are recorded in
[validation](testing/validation.md). These helpers remain Experimental and do
not close a compatibility owner.

## Experimental runtime information (2026-10-17)

`pg_is_in_recovery()`, `pg_jit_available()` and `pg_numa_available()` report
false for Cassie's current ready single-node profile: archive/standby recovery,
JIT execution and a NUMA runtime are absent. `unicode_version()` reports the
actual Rust standard-library Unicode table version. These values describe
Cassie's build and runtime; a PostgreSQL oracle build may report other values.
Only the Unicode version helper is eligible for immutable constant folding.

`pg_column_compression(any)` returns NULL for materialized ordinary SQL values,
including NULL. Arguments are still evaluated. This describes SQL value
representation, not Midge's physical block compression. An untyped direct
parameter rejects with native 42P18; selected explicit primitive parameters
retain their declared identities. Unknown literals and NULL are accepted.

`FROM pg_get_loaded_modules()` exposes module_name, version and file_name as
three TEXT columns, with no rows because Cassie has no PostgreSQL native-module
loader. Aliases, derived tables, CTEs, aggregation and outer joins use the normal
table-function path. Scalar target-list RECORD projection remains explicitly
unsupported (0A000); the FROM shape does not establish RECORD
ABI support. Compiled plans rebuild at epoch 16. No stored format changes.
These checks leave complete system-information, native catalog ABI and resource
qualification open. See [validation](testing/validation.md).

## Experimental option expansion (2026-10-17)

`FROM pg_options_to_table(text[])` expands selected one-dimensional logical
TEXT arrays into `option_name` and `option_value` TEXT columns. Splitting uses
only the first `=`; absent `=` produces NULL, while empty names/values,
whitespace, duplicates and quoted literal contents are retained. NULL/empty
arrays produce no rows. A NULL element raises 22004 without partial result rows.
Selected malformed array text raises 22P02.

The selected input path supports explicit one-dimensional TEXT[] constructors
with string/NULL literals, PostgreSQL array-literal casts and explicitly typed
TEXT parameters cast into arrays. Native TEXT[] parameter identity OID 1009
remains absent in this selected path: direct array parameters reject with 0A000 instead of
advertising the private logical-array identity as native. Higher dimensions,
explicit bounds, general dynamic constructors, native array ABI and SQL array
assignment remain pending. Stored logical arrays are verified through the public
ingest API followed by SQL reads; this does not qualify SQL array INSERT casts.

Aliases, CTEs, selected explicit/implicit lateral reads and empty outer joins use normal
relational execution. Scalar target-list RECORD projection remains explicitly
unsupported in the selected RECORD path; #817 owns remaining catalog/introspection
qualification. DISTINCT and in-call ORDER BY reject with native 42809.
Execution checks controls, admits argument scratch/output ownership and releases
owners on errors. Compiled plans rebuild at epoch 17; stored formats do not
change. Full resource/restart and owner qualification remain open. See
[validation](testing/validation.md).

## Experimental statement text (2026-10-17)

`current_query()` and `pg_catalog.current_query()` return the original executing
SQL as TEXT (OID 25, length/typmod -1) in text and binary formats. Simple Query
returns the entire frontend message, including all statements, comments and
whitespace. Extended Query returns the original Parse text across parameter
rebinding and suspended portal fetches. Embedded SQL and the public analyzed
EXPLAIN entry point retain their submitted text. Calls through views observe the
outer executing statement.

Statement text belongs to an immutable execution-local session clone. Concurrent
callers do not mutate shared session state. Text ownership and evaluator scratch
use query-memory admission; cancellation/error paths release that ownership.
Execution-result caching bypasses this helper, including calls through views;
compiled plans remain reusable and rebuild at epoch 18. Extra arguments reject
with native 42883. No persistent representation changes. This selected
primitive helper leaves complete session introspection and native OID/RECORD
types unqualified under the current feature and v1.0 qualification records.

`current_catalog`, `current_user`, `session_user`, `current_role`,
`current_database()`, and `pg_client_encoding()` return native `name` identity
(OID 19). `current_schemas(boolean)` returns native
`name[]` identity (OID 1003, element OID 19). `pg_client_encoding()` reports
the validated UTF8 session setting. NAME values reuse the existing string row
tag; general `name[]` parameters, constructors, operators, and assignment paths
remain unqualified.


Selected `pg_column_size(any)` support returns INT (OID 23). Explicit existing
fixed primitive casts use their declared widths, including SMALLINT/INTEGER/BIGINT
sharing the same Rust integer carrier. Unknown string literals include their
one-byte C-string terminator; explicit materialized TEXT/VARCHAR and BYTEA
literal or parameter casts include a four-byte uncompressed datum header.
UTF-8 byte lengths, NULLs, nested supported casts and text/binary wire formats
are covered against PostgreSQL 18.6. This describes selected SQL datums, not
Midge storage sizes. Direct typed parameters require explicit casts to preserve
identity. Unsupported stored varlena provenance, CHAR, NUMERIC, JSON, arrays,
vectors and unqualified expressions reject with `0A000` before evaluation;
these remain unsupported in this checkpoint. Untyped direct parameters
retain `42P18`. Query budget, cancellation and result ownership use existing
execution controls. Plan-cache cost model epoch is 19.
