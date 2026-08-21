# Catalog Support

Cassie's virtual catalogs describe Cassie objects for documented read-model workflows. They are
not PostgreSQL system catalogs, and PostgreSQL-internal catalog parity is not claimed. Only the two
views below are Stable. Every other `information_schema` or `pg_catalog` view remains Experimental
and may add or change columns before 1.0.

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

- `tests/catalog_introspection.rs` covers table discovery, complete column order after restart, and
  rename/drop visibility after restart.
- `tests/catalog_orm_metadata.rs` covers the complete supported column shape, defaults, nullability,
  and type metadata.
- `tests/compatibility_matrix.rs` verifies pgwire SQLSTATE `42P01` and relation metadata for an
  unavailable relation. The same error contract applies to unavailable virtual catalogs.
- Broader native pgAdmin-shaped queries remain deterministic repository tests, but pgAdmin and
  DBeaver certification remains separately tracked and is not promoted by this contract.
