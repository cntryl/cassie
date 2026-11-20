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
- The removed `CREATE GRAPH` backing tables. That feature was removed in
  v0.6.0; mapped property graphs read row-authoritative and column-authoritative
  tables alike.

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

Compact-field validation consumes the complete stored payload, including an
empty payload for SQL NULL. The shared integer tag does not erase the declared
SMALLINT, INT or OID range, and an extended value's stored type OID must match
the field's declared type OID. These checks preserve the existing field format
and matching-OID representation rules.

Validation follows the accessed source: point and index reads validate their
requested/candidate rows; projected reads decode their fetched fields. Full-table
traversal and authority conversion validate field-to-marker ownership across the
source table, including orphan fields that marker enumeration would otherwise
miss. The caller declares complete-source versus visited-row traversal before opening
its private cursor. An unrestricted SQL source and authority conversion validate
complete field ownership before returning their first rows. A bounded source
(such as LIMIT or EXISTS with an early stop) validates visited rows without a
whole-table ownership preflight. Result-admission caps and scan-buffer bounds do not select source intent; an
unrestricted logical source stays complete even when its physical result
transport is capped. Streaming LATERAL/derived sources inherit an outer early
stop, while blocking aggregate/order/distinct inputs still require complete
validation. Page size is not SQL source intent: paginating
an unrestricted portal still validates the complete source before its first
successful page. Ranking operators that consume all input retain complete-source
validation even when their output is top-k. Callback scans that may stop early
use visited-row intent unless their caller explicitly declares complete input.
Controlled traversal checks cancellation during streaming preflight and retains
no full-table field payload collection. Field-entry and marker-lookup validation
reads are counted separately from the existing visited-row marker counter. This scope was selected by
the delegated Jev review during the v0.5.0 closeout on 2026-11-14; its owning
issue and review receipt record the decision.

The read returns no partial result and never falls back to a row blob or a
derived index. `cassie verify` and the storage verifier report the same
conditions.

### Selected-field reads (#1356)

A read of a column table pays for the live marker plus the fields it selects.
The selection is resolved once per scan into a `ColumnFieldSelection` (the
non-retired schema fields in schema order whose lookup key is in the
projection; no projection selects every non-retired field). That primitive is
the reusable boundary for later batch and aggregate readers.

**Shapes that read only the selected fields.** Any scan whose physical plan
carries a field projection: plain column projections, with or without a
filter on plain columns, `ORDER BY` plain columns, `LIMIT`/`OFFSET`, and
expression projections that the typed projected-read path accepts (no
`ORDER BY`, enough transport budget). The cursor fetches one field key per
selected field per visited row. Unselected field keys are never fetched, so
an unselected payload costs neither a read, a decode, nor query memory.
Filters on a stored column select that column too.

**Fallbacks (read every non-retired field).** `SELECT *`, projections or
`ORDER BY` expressions the planner cannot reduce to a field list (for
example an expression projection with `ORDER BY`), grouped/`DISTINCT`/window/
set-operation sources, point loads by id for DML and constraint checks, and
index maintenance. A field added after a row was written and absent from the
row costs no payload read: it resolves from its default metadata. A field with
`ADD COLUMN ... DEFAULT <non-null>` is backfilled at ALTER time and is read
like any stored field.

**Ownership and marker validation without decoding.** Validation is separate
from selection and keeps the scope above. A complete-source traversal runs
the ownership preflight before the first row: it scans every non-retired
field's key range and probes the marker of each field key, so an orphan field
is found even when its field is not selected. It never decodes a payload.
A visited-row traversal (LIMIT, EXISTS with an early stop) reads the visited
marker, validates its marker length, and checks `field_is_required` only for
the selected fields, so corruption in unselected fields or in rows not
visited is not seen. A selected required field with no key fails with
`XX001` when that row is visited, before any later row is read, and no
unrelated field is fetched or decoded for it.

**Counters.** `Midge::column_field_reads_for_diagnostics()` returns a
`ColumnFieldReadSnapshot` with `payload_reads` (field payloads fetched),
`payload_bytes` (their raw size), `decodes` (payloads decoded into values) and
`retained_bytes` (the memory admitted for decoded selected values); use
`.since(earlier)` for a delta. They are independent of
`column_validation_reads_for_diagnostics()` (ownership: field entries and
marker lookups of the complete-source preflight) and
`query_scan_entries_for_diagnostics()` (visited live markers). A two-field
projection of a wide table reports the same `payload_reads`, `payload_bytes`,
`decodes` and `retained_bytes` as the matched narrow table; only the ownership
reads grow, by two per row per extra field. Growing only the unselected
payload tenfold leaves every selected-field counter unchanged.
`EXPLAIN` already reports `storage_mode=column-store`; the counters are the
per-field evidence.

**Budget.** The query memory budget is reserved for the row and output names
before the first field is fetched and grown by each selected field's decoded
size before that field is decoded, so a budget that fits the selected values
passes regardless of unselected payload size and a selected wide field is
rejected with a resource-limit error. Midge returns a fetched value as one
buffer, so the raw bytes of a selected field exist before the grow check; the
check precedes decode and retention.

### Versioning

- Layout level `cassie-midge-layout-v4-0.5.0-columnar` follows
  `v3-0.2.0-index-generation`. `require_storage_layout_version` durably raises
  the minimum-reader fence in the schema family as a prerequisite to the
  database-family transaction that publishes column data and catalog metadata.
  The fence is monotonic: it can remain at v4 after a failed, rejected or rolled
  back first CREATE/conversion, with no automatic downgrade. SQL rows and catalog
  metadata still publish atomically within their database family. A binary that
  does not know the raised level refuses startup without changing data. The
  [selected clarification](https://github.com/cntryl/cassie/issues/1105#issuecomment-6878076513)
  follows the existing generic monotonic/durable-fence and v4 ordering witnesses;
  it does not claim a separately executed v4 failed-SQL-rollback test.
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
- Cassie's query budget also admits source key/value copies and retained scan
  capacity before allocation, alongside rewritten values. Conversion checks
  interruption before opening and advancing source scans, during old-layout
  cleanup and before commit; an admission or interruption failure publishes
  no authority change.
- A crash before commit leaves the previous mode intact; WAL replay never
  applies a partial commit.

## Database images and restore

Logical database images copy every non-catalog key in the family, so they
carry `\x18` data. `collection-meta` carries the mode and `storage_version`.
Restore preserves mode, values, row ids and constraints. Derived indexes
restore with their data and remain rebuildable. An image whose header layout
level is newer than the binary supports is rejected before staging.
