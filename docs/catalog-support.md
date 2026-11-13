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

The active v0.2 dependent catalog field set selected on 2026-11-11 is recorded
below. The [2026-11-06 v0.2 catalog inventory](catalog-v020-inventory-2026-11-06.md)
is retained as historical implementation context; its wider inventory does not
expand the current milestone selection.

## Finite v0.2 dependent catalog columns (2026-11-11)

Issue [#817](https://github.com/cntryl/cassie/issues/817) selects only the
catalog fields required by [#818](https://github.com/cntryl/cassie/issues/818),
[#828](https://github.com/cntryl/cassie/issues/828), and
[#843](https://github.com/cntryl/cassie/issues/843). The fields below are a
finite descriptor and row contract, not a claim that every dependent feature
or PostgreSQL catalog behavior is implemented.

| Consumer | Catalog relation | Selected fields |
| --- | --- | --- |
| #818 object identifiers | `pg_namespace` | `oid`, `nspname`, `nspowner`, `nspacl` |
| #818 object identifiers | `pg_class` | `oid`, `relname`, `relkind`, `relam`, `relnamespace`, `relnamespace_oid`, `relowner`, `relhasindex`, `relpersistence`, `reltuples`, `relacl`, `reloptions` |
| #818 object identifiers | `pg_type` | `oid`, `typname`, `typnamespace`, `typlen`, `typbyval`, `typtype`, `typcategory`, `typelem`, `typarray`, `typbasetype`, `typrelid` |
| #818 object identifiers | `pg_attribute` | `attrelid`, `attrelid_oid`, `attname`, `attnum`, `atttypid`, `attnotnull`, `atttypmod`, `atthasdef`, `attisdropped`, `attidentity`, `attgenerated`, `attcollation` |
| #828 composites and domains | `pg_class` addition | `reltype` |
| #828 composites and domains | `pg_type` additions | `typnotnull`, `typtypmod` |
| #843 built-in extensions | `pg_extension` | `oid`, `extname`, `extowner`, `extnamespace`, `extrelocatable`, `extversion`, `extconfig`, `extcondition` |
| #843 built-in extensions | `pg_available_extensions` | `name`, `default_version`, `installed_version`, `comment` |
| #843 pgcrypto shim | `pg_proc` | Only the registered `pgcrypto.digest(text|bytea, text)` identities selected by #843; broad routine discovery is excluded. |

The `#828` additions remain pending behind its open #826 table/type lifecycle
prerequisite. The `#843` descriptors exist in
[`virtual_views_extensions.rs`](../src/catalog/virtual_views_extensions.rs),
but `pg_extension` and `pg_available_extensions` currently return no rows;
extension installation, availability, digest registration, and lifecycle are
still owned by #843. Empty rows do not advertise an installed or available
extension. The `#818` conversion and lookup contract is owned by #818 (privilege-aware
unqualified visibility is implemented and tested, see the
[object identifier contract](object-identifier-contract.md#visibility)); the base relation descriptors
do not establish that lookup behavior by themselves.

Evidence for the #843 catalog rows (executed in the final full run on
1d0750c19): descriptors in
[`catalog_descriptors.rs`](../tests/compatibility/lifecycle/extensions/catalog_descriptors.rs),
installed and available rows in
[`extension_catalog_coverage.rs`](../tests/compatibility/lifecycle/extensions/extension_catalog_coverage.rs),
and the registered `pgcrypto.digest` `pg_proc` rows in
[`pgcrypto_extension.rs`](../tests/compatibility/lifecycle/extensions/pgcrypto_extension.rs).

### #817 exclusions

The selected #817 catalog surface deliberately excludes the following; each is
an absent capability or documented divergence, not a regression. Details are in
the linked entries.

- Fields beyond the finite selection above, including `extconfig`,
  `extcondition` and the available-extension `comment`
  ([#843 field set](#843-extension-catalog-field-set--2026-11-11)); broad
  `pg_proc` (user routine rows, `proargtypes`) and `pg_stat_activity`.
- Owners and ACLs: `nspowner`/`relowner` describe `root`; `nspacl`/`relacl` are
  empty; routine owner checks are not enforced.
- `pg_class.reltype` is 0 for views (PostgreSQL gives a row type), and domain
  rows carry no `typdefault` or `pg_constraint` rows.
- `relhasindex` stays true after the last index is dropped (no VACUUM); see
  `pg_class.relhasindex` below.
- Cassie Column, TimeSeries and legacy `USING vector` indexes are not
  published as `pg_class` rows and have no access-method identity.
- `pg_identify_object`: select-list projection, `pg_identify_object_as_address`
  and `pg_get_object_address`; column names for indexes, materialized views and
  catalog relations; classes outside relations, types, schemas, routines,
  extensions and roles (`0A000`).
- No routine overloads within a schema; user-defined operators are not
  resolvable ([type contract](type-contract.md)).
- Custom array identities beyond the registered arrays (enum and composite array
  rows are registered with their type) and the absent object registries, which are
  shape-only empty relations. `pg_enum` is no longer one of them: named enums
  (#827) populate it under the [v0.4.0 named enum contract](type-contract.md#selected-v040-named-enum-contract-2026-11-13);
  that implementation is authored and pending the assembler gate.

In the current source, [`virtual_views_pg.rs`](../src/catalog/virtual_views_pg.rs)
declares and builds the selected base `pg_namespace`, `pg_class`, `pg_type`,
and `pg_attribute` rows. Their existence does not complete the dependent
workflows: user object resolution still belongs to #818, selected
domain and composite type rows are covered by executed #828 regressions; domain rows carry no
`typdefault` and no `pg_constraint` rows, and extension catalog rows
remain empty until #843. `nspowner` and
`relowner` currently describe the bootstrap `root`; `nspacl` and `relacl` are
empty. Role-aware schema/table grant reporting remains with #831.

The `pg_class.relam` identity follows PostgreSQL 18.6 for every published
relation kind: heap tables and SQL materialized views report OID 2, scalar
btree indexes OID 403, hash indexes OID 405, full-text indexes GIN OID 2742,
and installed pgvector HNSW/IVFFlat indexes the access-method OIDs registered
by the `vector` extension. Views, sequences and composite types report 0, as
PostgreSQL does. Cassie Column, TimeSeries and legacy `USING vector` derived
indexes are not published as `pg_class` rows, so they need no access-method
identity. Regressions executed and passing in the final full run on
1d0750c19 (12,657/12,657):
[`relation_access_methods.rs`](../tests/catalog_security/catalogs/types/object_identity/relation_access_methods.rs).

`pg_identify_object(classid, objid, objsubid)` is a FROM-only table function
returning `type`, `schema`, `name` and `identity` text columns with
PostgreSQL 18.6 spelling. It identifies the classes Cassie models:
`pg_class` relations (tables, views, materialized views, indexes, sequences
and composite-type relations), `pg_type` types (built-in, array,
composite, domain and table row types), `pg_namespace` schemas, `pg_proc` user and built-in routines,
`pg_extension` extensions and `pg_authid` roles. A sub-object number names a
column of tables, views, sequences and composite types; indexes, materialized
views and catalog relations name no column and return only the column type. The
sub-object number is `pg_attribute.attnum`: a dropped table column or composite
attribute keeps its number and is identified by PostgreSQL's
`........pg.dropped.N........` placeholder, and the numbers after it do not
shift. An argument may be a scalar subquery. A
missing object returns the class description with `NULL` columns, a `NULL`
argument returns an all-`NULL` row, and a class Cassie does not model fails
with SQLSTATE `0A000`. Projecting the record in a select list, and
`pg_identify_object_as_address` and `pg_get_object_address`, remain absent
capabilities rather than regressions. Regressions executed and passing in the
final full run on 1d0750c19 (12,657/12,657):
[`identify_object.rs`](../tests/catalog_security/catalogs/types/object_identity/identify_object.rs).

The selected `pg_class.relhasindex` value currently reports whether the
relation has an index, derived from live index metadata.
[PostgreSQL 18.6](https://www.postgresql.org/docs/18/catalog-pg-class.html)
defines a lazy flag that may remain true after the last index is dropped until
VACUUM clears it. Cassie's persisted stale-flag decision, with PostgreSQL 18.6
oracle evidence, is recorded under `pg_class.relhasindex` below.

Separate dependency question for coordinator triage: the [#954 JSONPath
cancellation fixture](../tests/pgwire_extended/types/json/jsonpath/cancellation.rs)
polls `pg_catalog.pg_stat_activity`, and Feature Support currently records
that view as a #817 prerequisite. The active #817 field set above does not name
`pg_stat_activity`, and current source registers only its builtin identities
in [`pg_class.tsv`](../src/catalog/builtin_identity/pg_class.tsv) and
[`pg_type.tsv`](../src/catalog/builtin_identity/pg_type.tsv), not a queryable
view. This field inventory does not remove or redefine the existing #954
dependency; ownership and scope need a separate disposition before that
cancellation fixture can rely on the view.

Do not add broad `pg_proc`, dependency, sequence, or partition catalog
behavior through this field inventory. Named enum rows (`pg_type` with
`typtype = 'e'`, `pg_enum`) belong to
[#827](https://github.com/cntryl/cassie/issues/827) and are specified by the
[v0.4.0 named enum contract](type-contract.md#selected-v040-named-enum-contract-2026-11-13); they are
implemented-pending-gate, not executed evidence. The exact
extension field shapes and the pinned PostgreSQL 18.6 observations are also
recorded in the [extension compatibility contract](extension-compatibility-contract-2026-11-09.md).

## Stable named-client subset

### `information_schema.tables`

| Column | Cassie contract |
| --- | --- |
| `table_schema` | Schema containing the Cassie table or view. |
| `table_name` | Local table or view name in the connected database. |
| `table_type` | `BASE TABLE` for Cassie tables or `VIEW` for Cassie views. |

Rows are scoped to the connected database and reflect create, rename, drop, and restart hydration.
The sqlx 0.8.3 and Diesel 2.2.6 retained probes use this view for named table discovery.
For selected bare, qualified and aliased predicates, an unspecified parameter
compared with `table_name` infers Cassie's declared `TEXT` type (OID 25).
PostgreSQL 18.6 instead infers its `name` type (OID 19); this does not claim
parameter-type identity parity. Qualified parameter inference across mixed
unaliased joins remains open under [#780](https://github.com/cntryl/cassie/issues/780).

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
- `should_preserve_database_scoped_information_schema_after_drop_restart` creates
  same-named tables with different metadata in two databases, renames and drops one,
  then reopens local storage in a child explicitly configured for local mode. It checks table
  and column absence only in the changed database and preserved type, default, nullability
  and ordinal values in the other.
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
partition/trigger/policy/event-trigger and parameter-ACL registries (the enum
registry, `pg_enum`, moved to real rows under #827). Empty
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

`pg_tables` and `pg_views` expose native NAME descriptors for their schema and
object names. Their owner fields remain NULL because relation ownership is not
stored. `pg_constraint.conenforced` is true for the constraints Cassie stores;
generated primary-key and NOT NULL names follow PostgreSQL's catalog naming
convention, while explicit names are preserved. The rest of its experimental catalog columns retain their current logical
carriers. `pg_proc` exposes the pinned built-in routine name, OID, and result
type rows used by the selected introspection helpers; it does not advertise
user-defined SQL routine identities. Built-in `pg_get_function_arguments`,
`pg_get_function_identity_arguments`, and `pg_get_function_result` resolve the
pinned built-in `pg_proc` identity table; user-defined routine signatures are
not implied by this helper. `pg_my_temp_schema()` returns OID 0 and
`pg_is_other_temp_schema(oid)` returns false because Cassie does not create
temporary schemas.

For scalar indexes, `pg_index.indpred` retains Cassie's experimental TEXT
(OID 25) descriptor. It returns SQL NULL when an index has no partial
predicate and the stored predicate text for a partial index. This qualifies
NULL/value semantics only; it does not expose PostgreSQL's `pg_node_tree`
representation or claim catalog ABI parity. Text and binary predicate values,
binary boolean values, and empty-result descriptors are covered by
[`result_framing.rs`](../tests/pgwire_extended/results/result_framing.rs).
An unavailable virtual catalog relation returns `42P01`, covered by
[`should_recover_after_undefined_pg_catalog_view_lookup`](../tests/pgwire_extended/catalogs/unavailable_virtual_view.rs).

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
rows, and broad OID/reg* lifecycle remain unqualified; current issue criteria
define active follow-up. Selected SQL materialized-view relation identity is
documented separately under Feature Support and does not promote this catalog
or the broader OID/reg* surface.
Native column names and selected values are independent pinned observations;
logical-carrier descriptors do not establish PostgreSQL descriptor ABI parity.
The experimental `pg_type.typarray` value links only scalar types whose array
OIDs are present in Cassie's finite built-in array registry; array types and
types without a registered built-in array report zero. This supports the
selected registered-array TypeInfo/COPY path and does not add custom array
type identities.
Executed counts and remaining failures are recorded in
[validation](testing/validation.md).

### Finite v0.2 dependent catalog columns 2026-11-11

The #817 catalog contract is limited to fields used by the named #818, #828,
and #843 workflows. The selected fields, in descriptor order, are:

- `pg_namespace`: `oid`, `nspname`, `nspowner`, `nspacl`.
- `pg_class`: `oid`, `relname`, `relkind`, `relam`, `relnamespace`,
  `relnamespace_oid`, `reltype`, `relowner`, `relhasindex`, `relpersistence`,
  `reltuples`, `relacl`, `reloptions`.
- `pg_type`: `oid`, `typname`, `typnamespace`, `typlen`, `typbyval`,
  `typtype`, `typcategory`, `typelem`, `typarray`, `typbasetype`,
  `typnotnull`, `typtypmod`, `typrelid`.
- `pg_attribute`: `attrelid`, `attrelid_oid`, `attname`, `attnum`,
  `atttypid`, `attnotnull`, `atttypmod`, `atthasdef`, `attisdropped`,
  `attidentity`, `attgenerated`, `attcollation`.
- `pg_extension`: `oid`, `extname`, `extowner`, `extnamespace`,
  `extrelocatable`, `extversion`; `pg_available_extensions`: `name`,
  `default_version`, `installed_version`.
- `pg_proc`: `oid`, `proname`, `prorettype`, `pronamespace` only for the
  installed pgcrypto SHA-256 digest shim. `proargtypes` and general routine
  introspection are not selected.

The PostgreSQL-facing `pg_class`, `pg_index`, and `pg_indexes` surfaces omit
Cassie's `Column` and `TimeSeries` derived-index kinds and its legacy `USING
vector` index method. Those are genuine Cassie runtime indexes, but they have
no PostgreSQL access-method identities; the catalogs therefore do not publish
index relations with `relam = 0` or invent `pg_am` rows for them. The selected
pgvector `USING hnsw` and `USING ivfflat` index methods are published only when
their matching installed extension identity exists. Their query/storage
behavior is unchanged. The `relhasindex` summary still reflects current or
recently dropped indexes of those kinds. PostgreSQL 18 defines `pg_class.relam` as a reference to
`pg_am.oid` and `pg_am` as one row per supported access method; see
[`pg_class`](https://www.postgresql.org/docs/18/catalog-pg-class.html) and
[`pg_am`](https://www.postgresql.org/docs/18/catalog-pg-am.html). Focused
catalog visibility coverage is authored but unexecuted in
[`catalog_information/semantics.rs`](../tests/catalog_security/catalogs/indexes/catalog_information/semantics.rs).
The selected #818 integration uses the same publication predicate for
`pg_class`, `pg_index`, `pg_indexes`, and `regclass` name/OID resolution:
internal Column, TimeSeries, and legacy `USING vector` index names remain
resolvable through Cassie's query/storage APIs, but not through the PostgreSQL
object-identity surface. An OID supplied directly still renders as its numeric
value when no published relation owns it. Jev selected this alignment with
probability 0.89 and confidence 0.85. A follow-up found that the original
predicate admitted legacy `USING vector` rows with `relam = 0` (gap probability
0.90); Jev selected publication only for HNSW/IVFFlat rows with installed
identities (probability 1.0, confidence 1.0). Authored regressions cover
Column/TimeSeries/legacy-vector name and OID lookup plus installed HNSW
identity; they remain unexecuted. The requests and responses are retained under
ignored `target/v020-evidence/818-object-scope/` and
`target/v020-evidence/844-vector-schema/`.

Catalog row generation uses the same effective database as binding and
`regclass` resolution. `CassieSession::new(user, None)` is a supported embedded
session shape accepted by `Cassie::execute_sql`; query binding falls back to
Cassie's configured default database while the SQL `current_database()` value
remains empty. Jev selected preserving that empty function result (probability
0.96, confidence 0.95): the absent stored session database remains observable,
while the default is only the query execution and name-resolution fallback.
The execution source therefore passes the effective default
database into virtual catalog filtering, so another database's index cannot
appear in `pg_class`, `pg_index`, or `pg_indexes` when `regclass` resolves only
within the default database. Jev initially selected no reachable mismatch from
a prompt that omitted this constructor; that no-defect judgment was based on
incomplete source facts and was superseded. The preceding adversarial review's
0.54 catalog-divergence probability prompted the source trace, which found the
public path; a corrected follow-up selected passing the effective database to
virtual row generation (probability and confidence 1.0). The regression in
[`database_scope.rs`](../tests/catalog_security/namespaces/database_scope.rs)
covers all three catalog views, `current_database()`, direct `to_regclass`,
and foreign/default-database index controls through the public embedded-session
API; it is authored but unexecuted.
Both Jev request/response pairs are retained under ignored
`target/v020-evidence/818/`; the triggering 0.54 response is in
`target/v020-evidence/844-vector-schema/jev-vector-source-gap-focused-followup-response.json`.

For the #818/#831 interaction, unqualified `regclass` name resolution uses the
effective search path after schemas without USAGE are excluded. Once a schema
has USAGE, resolving a relation OID does not require table SELECT; actual
relation reads still do. Explicitly schema-qualified `regclass` lookup and
`regnamespace` lookup resolve an existing identity without schema USAGE. Jev
selected the unqualified-search-path rule (probability 0.95, confidence 0.93),
direct namespace identity behavior (0.90, 0.85), and qualified lookup behavior
(0.97, 0.96). This follows PostgreSQL 18's documented search-path filtering and
`regclass` behavior; see [search_path](https://www.postgresql.org/docs/18/runtime-config-client.html),
[object identifier types](https://www.postgresql.org/docs/18/datatype-oid.html),
and the [PostgreSQL 18 `regclass` implementation](https://github.com/postgres/postgres/blob/REL_18_STABLE/src/backend/utils/adt/regproc.c).
The role-aware lookup regressions are executed and passing
([`identifier_visibility.rs`](../tests/catalog_security/security/object_privilege_grants/identifier_visibility.rs), including user routines and types); they distinguish unqualified lookup before/after USAGE, qualified
identity lookup, namespace lookup, and table SELECT authorization.

The descriptor additions selected by #828 are present: rows for the currently
supported types report `typnotnull = false` and `typtypmod = -1`, and
`pg_class.reltype` is the derived row-type OID for tables (see below) and 0 for
views and sequences. User composite types add a `pg_type` row
(`typtype = 'c'`, `typcategory = 'C'`, `typrelid`, `typarray`), a `_name`
array `pg_type` row, a `pg_class` row with `relkind = 'c'` whose `reltype` is
the type OID, and one `pg_attribute` row per field. These rows are covered by
the #828 regressions executed in the final full run on 1d0750c19. Increment 2 adds a row type per table: a `pg_type` row (`typtype = 'c'`, `typrelid` = the
table) and `_name` array row with OIDs allocated by the Midge object-id allocator
and persisted on the table (below `2^31`, stable across rename and restart), and `pg_attribute` rows of a
composite type use physical attribute numbers, so a dropped attribute leaves a gap.
Row-type rows are listed from each table's canonical name: two schemas holding
the same table name publish one row type each, `typnamespace` is the table's
own schema, and only the tables of the connected database are listed
(authored regressions, execution deferred to the batch gate:
[`row_type_catalog_scope.rs`](../tests/catalog_security/catalogs/types/object_identity/row_type_catalog_scope.rs)).
An index's `pg_class.oid` is assigned by the same allocator at `CREATE INDEX`
([Object Identifier Contract](object-identifier-contract.md#name-resolution-and-spelling-rules)).
Domain types add a `pg_type` row (`typtype = 'd'`,
`typbasetype`, `typtypmod`, `typnotnull`) and a `_name` array row with allocator-backed
OIDs; `pg_attribute.atttypid` of a domain column is the domain OID while `atttypmod` is
`-1`. Enum types add a `pg_type` row (`typtype = 'e'`, `typcategory = 'E'`,
`typlen = 4`) and a `_name` array row, and `pg_enum` lists `oid` (the immutable member
ID), `enumtypid`, `enumsortorder` and `enumlabel` per the [v0.4.0 named enum contract](type-contract.md#selected-v040-named-enum-contract-2026-11-13)
(authored under #827, pending the assembler gate); broad routines,
dependency families, sequences, and partitions remain unselected except for
separately documented finite workflows.

Jev weakly selected exposing the descriptor and zero sentinel for `reltype`
(probability 0.57, confidence 0.35). PostgreSQL 18 documents `reltype` as the
row type OID when one exists and zero for indexes and sequences; Cassie
currently has no row-type identity for its table/view relations, so zero records
that absence and does not claim PostgreSQL's automatic composite row types.
Evidence: [PostgreSQL 18 `pg_class`](https://www.postgresql.org/docs/18/catalog-pg-class.html)
and the planned type identity boundary in [#828](https://github.com/cntryl/cassie/issues/828).

`pg_class.relhasindex` reports true while an index exists and stays true after
committed index cleanup, matching PostgreSQL's “has or recently had” flag.
Cassie stores that history with the owning collection metadata in the same
database-family transaction as the cleanup record. It survives restart and
transaction rollback discards it with the rest of the overlay. Cassie has no
selected VACUUM operation, so the flag remains true until the relation is
dropped.

Oracle verification (2026-11-12, local Homebrew PostgreSQL 18.6, throwaway
cluster): after `CREATE TABLE t2(a int); CREATE INDEX i2 ON t2(a); DROP INDEX i2`,
`SELECT relhasindex FROM pg_class WHERE relname = 't2'` returns `t`; after a
subsequent `VACUUM t2` it returns `f`. Cassie matches the first observation and
has no VACUUM that could produce the second, so the only divergence is the
absence of the clearing operation, which is an unselected maintenance feature
and not a defect. Decision: keep the sticky flag; the lifecycle regressions in
`type_catalog_fields.rs` (restart persistence, rollback) are the executable
evidence. Tracked as a residual of #817 (v0.3.0), which remains open.

The earlier lifecycle probes are authored but unexecuted pending the
coherent bundle gate. Jev selected a persisted history bit over deriving only
from current indexes or the cleanup tombstone (probability 0.85, confidence
0.77); the latter disappears when Midge completes cleanup and cannot survive a
restart. The implementation updates the bit in the existing Midge data
transaction, which is staged by the active database-family overlay. Regressions
in [`type_catalog_fields.rs`](../tests/catalog_security/catalogs/columns/type_catalog_fields.rs)
cover restart persistence and rollback, but have not been executed. PostgreSQL
documents the row-type reference and lazy `relhasindex` clearing behavior in
[`pg_class`](https://www.postgresql.org/docs/18/catalog-pg-class.html). The
non-domain `typnotnull` and `typtypmod` defaults are described in
[`pg_type`](https://www.postgresql.org/docs/18/catalog-pg-type.html).

### #817 stable attribute identity and type collation — 2026-11-11

For persisted physical tables, `pg_attribute.attnum` follows Midge's stable
`RowSchema.field_id`; dropped slots remain visible at the same positive number
with PostgreSQL's dropped marker name, `attisdropped = true`, and `atttypid = 0`.
Dropped rows clear not-null, default, identity, and generated metadata while
retaining the stored type modifier and type-derived collation. Views and SQL
materialized views continue to use their derived output ordinals. Explicit
column `COLLATE` metadata remains unsupported by table DDL.

The finite collation mapping is OID 100 (`default`, provider `d`) for TEXT,
CHAR, VARCHAR, and arrays of those types; OID 950 (`C`, provider `c`) for NAME;
and zero for the other selected field types. `pg_collation` exposes the
referenced `default`, `C`, and existing `POSIX` identities. This supplies
truthful selected catalog values without selecting native catalog descriptor
parity or arbitrary per-column collations.

Jev selected this collation mapping with probability 0.99/confidence 0.99 and,
after source review plus authored schema-evolution and local-restart probes,
found no remaining source-backed gap in this finite slice (0.97). The probes
remain unexecuted pending the consolidated ordered validation gate. Key-free
Jev evidence is retained under
`target/v020-evidence/817-stable-attribute-identity/`.

The index history contract above is persisted in current collection metadata;
it does not add compatibility or migration behavior between unreleased builds.

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
the selected cancellation witness checks already-canceled execution and does
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
(OID 19). Explicit NAME casts, assignments, parameters, and results preserve
that identity in text and binary formats; text input truncates at a complete
UTF-8 boundary of 63 bytes and binary input rejects longer values.
`current_schemas(boolean)` returns native `name[]` identity (OID 1003, element
OID 19). The selected one-dimensional NAME[] text/binary parameter, assignment,
and result paths reuse the existing array carrier. NAME[] text input uses
PostgreSQL array syntax, direct array text results retain Cassie's JSON-array
spelling, and explicit casts to text use PostgreSQL array text; binary values
use PostgreSQL array framing. `pg_client_encoding()` reports the validated
UTF8 session setting. NAME values reuse the existing string row tag; no
persistent row layout changes. Locale-sensitive catalog ordering and wider
NAME[] bounds/assignment semantics remain pending.
`SET ROLE` and `SET SESSION AUTHORIZATION` return `0A000`; authenticated role
switching and restricted-role privilege matrices remain under #831.
PostgreSQL backend-statistics reset helpers also return `0A000`: Midge does not
maintain PostgreSQL backend statistics for Cassie to reset.


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


## Selected v0.2 grants, extensions and comments (2026-11-04)

The user approved this contract direction on 2026-11-04. It is selected
implementation scope, not implemented or qualified support. Existing rejection
and compatibility boundaries remain until the owning acceptance cases pass.

### #831

Retain current password login and Argon2 hashes; root remains the administrator.
Operators create separate LOGIN roles directly, without memberships/inheritance. Root
alone manages roles, grants and DDL. Select CONNECT, schema USAGE and table
SELECT/INSERT/UPDATE/DELETE with explicit per-role grants/revocations; no PUBLIC
defaults or grant options. Applications log in directly as their chosen actor; keep SET
ROLE and SET SESSION AUTHORIZATION deterministically unsupported in this finite profile.
Projector roles get required DML, reader roles SELECT only. Require SELECT for read
columns/RETURNING/subqueries, USAGE for accessed schemas and the relevant write
privilege for mutations/COPY. Privilege catalogs and has_* answers derive from the same
authority. No RLS, sequence/routine/default privileges or PostgreSQL membership parity.

Keep existing global login identity/password records and CONNECT behavior; store new
database-scoped grants as versioned records keyed by immutable grantee and
schema/relation object IDs in the owning family. Deny absent grants. Rename preserves
grants, drop cleans them, and revoked grants affect the next statement of an existing
session; already-admitted portal ownership needs an explicit revalidation boundary. No
cross-database transaction is implied by global role creation. Reconstruct grants on
restart and retain them in images; missing/corrupt grant metadata fails closed. New
public AST/error/privilege descriptors require review before implementation; retain
42501 for denied access.

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

#### #843 extension catalog field set — 2026-11-11

Keep the extension catalog surface finite to the named shim workflows. The
selected `pg_extension` fields, in descriptor order, are `oid`, `extname`,
`extowner`, `extnamespace`, `extrelocatable`, and `extversion`. The selected
`pg_available_extensions` fields are `name`, `default_version`, and
`installed_version`. Advertise only `pgcrypto` 1.4 and `vector` 0.8.7; their
`installed_version` is NULL until that extension is installed in the active
database. `extconfig`, `extcondition`, and the available-extension `comment`
are outside this profile.

For #817's selected routine identity, retain the current `pg_proc` fields
`oid`, `proname`, and `prorettype`, and add only `pronamespace`. Register the
two SHA-256 `digest` overload identities in the selected extension schema only
while `pgcrypto` is installed. Their OIDs and namespace identify the
registered rows; `proargtypes` and broader routine introspection remain
unsupported. SQL overload resolution still accepts only the selected TEXT and
BYTEA inputs with the `sha256` algorithm.

This finite field contract satisfies the catalog-support prerequisite named by
#843 for its selected extension workflows. The historical #843 triage note saying
that runtime work is blocked by #817 predates this selection; #817 remains open
for its broader runtime acceptance and evidence. This resolution authorizes only
the bounded #843 slice and does not claim #817 is complete.

The field set was selected after reconciling the staged descriptor probe in
`tests/compatibility/lifecycle/extensions/catalog_descriptors.rs` with the
current #843/#817 workflow scope. It intentionally does not expose the full
PostgreSQL 18 extension catalog shapes documented in
[`pg_extension`](https://www.postgresql.org/docs/18/catalog-pg-extension.html),
[`pg_available_extensions`](https://www.postgresql.org/docs/18/view-pg-available-extensions.html),
or [`pg_proc`](https://www.postgresql.org/docs/18/catalog-pg-proc.html).

### #845

The contract decision, object-address rules, unsupported boundary, PostgreSQL 18
semantics, recovery behavior, and planned regression evidence are recorded in
[`SQL object comments — issue #845`](sql-object-comments-issue-845.md). The
previous empty-string-retained note is superseded: NULL and empty string both
remove the comment. Runtime and recovery qualification remain pending; the
recorded Jev judgment is a lead, not proof. No pre-release migration or
old-reader compatibility is required; see [Release Baseline](release-baseline.md#pre-release-format-and-api-policy-2026-11-10).
