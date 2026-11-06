# v0.2 catalog dependency inventory

This is source-backed planning for #817 and the dependent work in #818, #828,
and #843. It does not claim those issues complete or expand Cassie's finite
catalog ABI. Named enum issue #827 is deferred to v0.4; no enum DDL or rows are
selected by this increment. The listed fields are the current finite baseline,
not an exhaustive selection for the future composite/domain or extension
contracts. Those owners must document their additional required fields and
identity/lifecycle rules before implementing dependent catalog rows.

| Relation | Current finite fields and owning source | Open boundary |
| --- | --- | --- |
| `pg_type` | `oid`, `typname`, `typnamespace`, `typlen`, `typbyval`, `typtype`, `typcategory`, `typelem`, `typarray`, `typbasetype`, `typrelid`; `virtual_views_pg::type_schema` and `pg_type` | #818 uses built-in type identity. #828 must select persisted composite/domain identities, base and relation links, and ownership before advertising such types. #843/#844 must select extension type identity without fabricating installed capabilities. |
| `pg_namespace` | `oid`, `nspname`, `nspowner`, `nspacl`; `virtual_views_pg::namespace_schema` | #818 name/OID resolution and current-database scope; #843 schema placement and transactional lifecycle. Current `oid` carrier is experimental INT rather than claiming native catalog ABI. |
| `pg_class` | `oid`, `relname`, `relkind`, `relam`, `relnamespace`, `relnamespace_oid`, `relowner`, `relhasindex`, `relpersistence`, `reltuples`, `relacl`, `reloptions`; `virtual_views_pg::class_schema` | #818 relation identity; #828 relation row types depend on #826. Existing `relnamespace` is a TEXT name with the OID exposed separately as `relnamespace_oid`; do not silently rewrite this established experimental shape. |
| `pg_attribute` | `attrelid`, `attrelid_oid`, `attname`, `attnum`, `atttypid`, `attnotnull`, `atttypmod`, `atthasdef`, `attisdropped`, `attidentity`, `attgenerated`, `attcollation`; `virtual_views_pg::attribute_schema` | #828 field identity/order and relation lifecycle. Existing `attrelid` is a TEXT relation name with `attrelid_oid` separately; do not claim native OID descriptor parity. |
| `pg_enum` | Empty registry schema `oid`, `enumtypid`, `enumsortorder`, `enumlabel`; `virtual_views_empty` | Named enums remain absent and deferred under #827; empty rows do not implement enums. |

#843's finite `pgcrypto`/`vector` extension identities require separate
extension catalog and lifecycle work, including `pg_available_extensions` and
`pg_extension`. Their identity, supported fields, schemas, ownership and
rollback behavior remain issue #843 obligations; this inventory does not
manufacture their rows. Likewise #828's relation and domain semantics cannot be
made complete by a catalog-only change or by model judgment.

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
