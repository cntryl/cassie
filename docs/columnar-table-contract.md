# Column-Authoritative Table Contract (v0.5.0)

Owner: [#1105](https://github.com/cntryl/cassie/issues/1105). Baseline: `main` @ `e949a6689`.
Implementation owners: #1106 (DDL and catalog), #1107 (layout and recovery),
#1108 (DML and transactions), #1109 (queries), #1110 (constraints), #1111
(derived indexes) and #1112 (conversion and images).

Public SQL, persistent format and conversion were approved by the owner on
2026-11-13. The constraint, index, query-surface, corruption and versioning
rows were selected with Jev 1.14.0. The request and response are in the
worktree's ignored `target/jev/columnar_contract_*` files. This document is
the design authority for the milestone.

## Storage modes

Every SQL table has exactly one authoritative storage mode:

| Mode | Selector | Source of truth |
| --- | --- | --- |
| Row (default) | No selector, `USING heap`, `WITH (storage = row_store)` | One CRB1 row blob per row in family `\x10` |
| Column | `USING columnar`, `WITH (storage = column_store)` | One field key per field per row in family `\x18` |

- `CREATE TABLE` without a selector stays row-authoritative. Existing tables
  never change mode implicitly.
- `USING columnar` and `WITH (storage = column_store)` are equivalent. If both
  are given and they disagree, the statement fails with `42601` before
  publication. `USING columnar` is also accepted by `CREATE TABLE ... AS`.
- `CREATE INDEX ... USING column` is a derived accelerator over a row table
  (`pg_table_storage` reports `column-indexed`). It never selects authority.
- Every other access method or `storage` value fails with `0A000` and
  publishes nothing.

### Catalog visibility

- `pg_am` gains row `(oid 9301, amname 'columnar', amtype 't')`. `heap` keeps oid 2.
- `pg_class.relam` is 2 for row tables and 9301 for column tables.
- `pg_table_storage.storage_mode` reports `row-store`, `column-indexed` or
  `column-store`.
- Text `EXPLAIN` reports `storage_mode=column-store`, the same spelling as
  `pg_table_storage`. The structured form reports `"Storage": "columnar"` or
  `"row"` for the authority and a separate `"Storage accelerator"` attribute for
  a derived column-batch index.

## One logical database, mixed modes

Row and column tables can coexist in one logical database and in one
statement or transaction. Both modes live in the same `db-*` Midge family, so
one Midge transaction commits every change to both modes together. The
[database family boundary](database-families.md) is unchanged: a transaction
still cannot span databases.

## Row identity, types, NULLs and order

- Row identity is Cassie's string row id: a UUIDv7 when not supplied. It is
  the same for both modes and independent of the primary key.
- Each field is stored by its stable `RowSchema` field id with
  `row_blob::encode_compact_value`. Every SQL type supported by row tables is
  supported, including numeric, enum, composite, domain, array, JSON and vector.
- SQL NULL is an explicit tagged field value, distinct from JSON `null`.
- A field added by `ALTER TABLE ... ADD COLUMN` after a row was written
  resolves from the column's stored default metadata, as for row tables.
- Unordered scans return rows in row-id order. As for row tables, SQL result
  order is guaranteed only by `ORDER BY`.

## Supported SQL

Column tables support the same surface as row tables:

- **Queries**: every SELECT shape supported for row tables, including joins in
  either orientation with row tables, aggregates, windows, CTEs, subqueries,
  set operations and pagination. Results equal the row-table results for the
  same logical data.
- **DML**: INSERT (including `ON CONFLICT`), UPDATE, DELETE, `INSERT ... SELECT`,
  `COPY FROM`, `COPY TO` and TRUNCATE, in autocommit and explicit
  transactions, with savepoints.
- **DDL**: the supported ALTER TABLE forms, rename, schema move, DROP, and
  `CREATE TABLE ... LIKE`. `LIKE` copies columns and selected options only;
  the new table's mode comes from its own selector.
- **Constraints**: NOT NULL, CHECK, DEFAULT, `GENERATED ... AS IDENTITY`,
  PRIMARY KEY, UNIQUE, domains, and FOREIGN KEY with a column table as either
  the referencing or the referenced side. Semantics and SQLSTATEs equal those
  of row tables.
- **Derived indexes**: scalar (btree, unique, partial, expression; hash is
  rejected with `0A000` as for row tables), full-text BM25, vector (HNSW,
  IVFFlat), hybrid retrieval and time-series. They are built and maintained
  from authoritative field values.

Generated expression columns remain unsupported, as for row tables.
`LIKE ... INCLUDING GENERATED` is an accepted no-op when there is no generated
expression to copy.

Not supported, rejected with `0A000` before any change:

- `CREATE INDEX ... USING column` on a column table, because it is already
  column-authoritative.
- Any table access method other than `heap` and `columnar`.
- Graphs. `CREATE GRAPH` node and edge backing tables are always
  row-authoritative, and `ALTER TABLE ... SET ACCESS METHOD columnar` on one
  fails. Graph traversal does not need column storage (owner direction,
  2026-11-13).

## Write and transaction semantics

Column tables use Cassie's existing session staging, savepoints, publication
and [pgwire transaction contract](pgwire-transaction-contract.md):

- Read-your-writes, rollback, savepoints and failed-transaction state behave
  as for row tables.
- REPEATABLE READ commit fencing covers the column live-marker key
  (`[rel, 'r', row_id]`) exactly as it covers the row key.
- A commit that touches both modes is one Midge transaction: all affected
  authoritative state commits, or none does.
- Conflicts use the existing `AbortOnWriteConflict` policy and retry rules.

**No-row-blob invariant.** No ingress path may write a `\x10` row blob or a
legacy document key for a column table. This covers INSERT, UPDATE, DELETE,
COPY, TRUNCATE, the time-series writers, fresh-document writes,
schema and column-type rewrites, and conversion. No read path may consult a
row blob for a column table. A focused test scans the family's raw keys after
each ingress path to prove it.

## Persistent format

Keys, all in family `\x18` with `rel` = `CollectionMeta.storage_id`:

| Key | Meaning |
| --- | --- |
| `[rel, 'r', row_id]` | Live marker; value is the row's `RowSchema` version (`u32` BE) then a write revision (`u64` BE) that every write increments |
| `[rel, 'f', field_id_be32, row_id]` | One compact-encoded value per non-retired field |
| `[rel, 'x', row_id]` | Deleted marker; value is the revision of the delete (`u64` BE), so a re-insert continues the revision; removed when the row is re-inserted |

The keys are field-major, so each column is one contiguous key range.
A write stores every non-retired field: a field the row omits is an explicit
SQL NULL value (`0x00`), so a field key is missing only for a field added
(`RowFieldMeta.added_version`) after the row's marker version. The revision
makes every update change the marker value, which is what a REPEATABLE READ
fence compares.

### Validation and corruption

A read fails the statement with SQLSTATE `XX001` (`data_corrupted`), naming the
relation and row id, when:

- a live marker exists but a non-retired field written at or before the
  marker's schema version has no field key;
- a field key exists for a row with no live marker;
- a value fails to decode or does not match the field's declared type.

Validation follows the accessed source: point and index reads validate their
requested/candidate rows; projected reads decode their fetched fields. Full-table
traversal and authority conversion validate field-to-marker ownership across the
source table, including orphan fields that marker enumeration would otherwise
miss. Controlled traversal checks cancellation during this streaming preflight
and retains no full-table field payload collection. This scope was selected by
the delegated Jev review during the v0.5.0 closeout on 2026-11-14; its owning
issue and review receipt record the decision.

The read returns no partial result and never falls back to a row blob or a
derived index. `cassie verify` and the storage verifier report the same
conditions.

### Versioning

- Layout level `cassie-midge-layout-v4-0.5.0-columnar` follows
  `v3-0.2.0-index-generation`. `require_storage_layout_version` raises it in
  the transaction that publishes the first column table. A binary that does
  not know the level refuses to start without changing data.
- `CollectionMeta.storage_version` is `1` for this format. Hydration rejects an
  unknown version with a startup error naming the table.
- Cassie is pre-release: no migration of earlier development stores is provided.

## Conversion

`ALTER TABLE name SET ACCESS METHOD columnar | heap`:

- Rewrites every row into the target mode and updates `CollectionMeta` in one
  database-family Midge transaction. Readers see the old or the new authority,
  never a mix. The table keeps its OID, row ids, schema, constraints, grants
  and comments.
- Derived indexes stay valid without a rebuild: they are keyed by field
  values and row ids, which conversion preserves. A `USING column` index blocks conversion to `columnar` with
  `0A000` until it is dropped.
- Same-mode conversion is a no-op.
- Converting is transactional DDL: it is allowed inside an explicit
  transaction and rolls back with it.
- The rewrite is bounded by Midge's transaction memory and spill limits. If
  they are exceeded the statement fails with `54000` and nothing changes.
- A crash before commit leaves the previous mode intact; WAL replay never
  applies a partial commit.

## Database images and restore

Logical database images copy every non-catalog key in the family, so they
carry `\x18` data. `collection-meta` carries the mode and `storage_version`.
Restore preserves mode, values, row ids and constraints. Derived indexes
restore with their data and remain rebuildable. An image whose header layout
level is newer than the binary supports is rejected before staging.
