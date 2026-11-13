# v0.2 catalog dependency inventory

This is source-backed planning for #817 and the dependent work in #818, #828,
and #843. It does not claim those issues complete or expand Cassie's finite
catalog ABI. Named enum issue #827 was deferred to v0.4 and is now specified by the
[v0.4.0 named enum contract](type-contract.md#selected-v040-named-enum-contract-2026-11-13);
no enum DDL or rows were selected by the v0.2.0 increment this note inventories. The listed fields are the current finite baseline,
not an exhaustive selection for the future composite/domain or extension
contracts. Those owners must document their additional required fields and
identity/lifecycle rules before implementing dependent catalog rows.

| Relation | Current finite fields and owning source | Open boundary |
| --- | --- | --- |
| `pg_type` | `oid`, `typname`, `typnamespace`, `typlen`, `typbyval`, `typtype`, `typcategory`, `typelem`, `typarray`, `typbasetype`, `typrelid`; `virtual_views_pg::type_schema` and `pg_type` | #818 uses built-in type identity. #828 must select persisted composite/domain identities, base and relation links, and ownership before advertising such types. #843/#844 must select extension type identity without fabricating installed capabilities. |
| `pg_namespace` | `oid`, `nspname`, `nspowner`, `nspacl`; `virtual_views_pg::namespace_schema` | `oid` and `nspname` now describe OID (26) and NAME (19), respectively. `nspowner` remains experimental INT and `nspacl` experimental TEXT; neither claims PostgreSQL owner or ACL semantics. #818 name/OID resolution and current-database scope; #843 schema placement and transactional lifecycle. |
| `pg_class` | `oid`, `relname`, `relkind`, `relam`, `relnamespace`, `relnamespace_oid`, `relowner`, `relhasindex`, `relpersistence`, `reltuples`, `relacl`, `reloptions`; `virtual_views_pg::class_schema` | #818 relation identity; #828 relation row types depend on #826. Existing `relnamespace` is a TEXT name with the OID exposed separately as `relnamespace_oid`; do not silently rewrite this established experimental shape. |
| `pg_attribute` | `attrelid`, `attrelid_oid`, `attname`, `attnum`, `atttypid`, `attnotnull`, `atttypmod`, `atthasdef`, `attisdropped`, `attidentity`, `attgenerated`, `attcollation`; `virtual_views_pg::attribute_schema` | #828 field identity/order and relation lifecycle. Existing `attrelid` is a TEXT relation name with `attrelid_oid` separately; do not claim native OID descriptor parity. |
| `pg_enum` | Empty registry schema `oid`, `enumtypid`, `enumsortorder`, `enumlabel`; `virtual_views_empty` | v0.2.0 baseline only. Under the v0.4.0 contract (#827, authored, pending the assembler gate) `pg_enum` lists real member rows and is no longer an empty registry. |

#843's finite `pgcrypto`/`vector` extension identities require separate
extension catalog and lifecycle work, including `pg_available_extensions` and
`pg_extension`. Their identity, supported fields, schemas, ownership and
rollback behavior remain issue #843 obligations; this inventory does not
manufacture their rows. Likewise #828's relation and domain semantics cannot be
made complete by a catalog-only change or by model judgment.

The live #843 issue references several extension acceptance files that are
absent from this source tree. The registered extension test currently present,
`tests/compatibility/lifecycle/extensions/dependency_targets.rs`, asserts that
`CREATE EXTENSION pgcrypto` is unsupported; it is a boundary witness, not
evidence that installation or the catalog contract works. The catalog identity
TSV files contain PostgreSQL names for `pg_extension` and
`pg_available_extensions`, but a name in that identity dataset does not supply
a queryable relation, installed extension state, or implementation. Before
changing that rejection, author focused create, lifecycle, catalog, rollback,
schema and digest regressions against the selected #843 profile. Preserve the
boundary test until the corresponding runtime behavior is implemented, then
replace its obsolete expectation rather than claiming it passes unchanged.

## Materialized view vector identity increment

The existing experimental scalar vector registry derives synthetic `vector(n)`
rows only for dimensions referenced in the current database. Ordinary view
output schemas participate, and `pg_class`/`pg_attribute` also expose SQL
materialized views. An unpopulated materialized view can be the only published
reference to a scalar vector width; its output-schema type must be visible in
`pg_type` and removed once the last published reference is dropped. Deduplicate
widths and exclude projection metadata that is not a SQL materialized view.
This repairs the existing synthetic Cassie contract; it does not advertise
PostgreSQL/pgvector extension parity or new durable representation.

The authored lifecycle and database-isolation probes in
`tests/catalog_security/catalogs/types/catalog_discovery/semantics.rs` were
written before this implementation. A rollback probe was added following review,
and a disk-backed isolated-child restart probe retains real recovery assertions.
All new probes **have not been executed**. No baseline RED or passing gate is
claimed. Full locked tests, strict Clippy, formatting,
test-policy checks and integrated final-head review remain deferred under the
current assembly-line instruction.

## Role OID name lookup increment

`pg_get_userbyid` is used by the selected catalog-discovery workflow. The old
implementation returned `root` for every input, including NULL and arbitrary
other role OIDs. The selected increment resolves known IDs from the same scoped
catalog used by `to_regrole`, returns SQL NULL for NULL, and renders a missing
ID as `unknown (OID=<unsigned>)`. OID integer/text conversion uses the existing
`oid` range rules. The bootstrap role remains `root` at OID 10. A missing
session catalog returns the existing object-lookup error instead of inventing a
role. This does not implement role-level privilege enforcement or close #831.

A local PostgreSQL 18.6 oracle returned NULL for `NULL::oid`,
`unknown (OID=0)` for `0::oid`, `unknown (OID=4294967295)` for both the
maximum OID and integer `-1`, and the actual login name for its existing role.
The temporary isolated oracle was stopped. PostgreSQL returns NAME OID 19 for
this function; Cassie's existing experimental function retains its TEXT result
descriptor (OID 25), so this increment does not claim descriptor ABI parity.
The authored root/NULL/unknown and autocommit create/drop regressions were
written before implementation and **have not been executed**. Role lookup uses
the attached session catalog rather than directly selecting the current command
catalog; transaction and command-snapshot visibility have not been established.
Malformed/overloaded parameter binding and broad lifecycle/privilege
qualification remain open under #817, #818 and #831.

The registered JSONPATH array identity remains visible in the compatibility
catalog. That identity does not promise SQL array construction or binary codec
support; the type contract and codecs separately reject those forms. Removing
the registered identity would change the existing catalog contract rather than
implement the unsupported operations.
