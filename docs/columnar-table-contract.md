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
| `[rel, 's', start]` | Derived pruning summary of one segment (see below); never authority. `start` is `0x01` for the table's first segment, else `0x02` then the first row id |

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
traversal validates field-to-marker ownership of the fields it reads across the
source table, including orphan fields that marker enumeration would otherwise
miss (owner decision D1, #1358: a complete-source scan validates the fields it
reads plus the markers; an orphan in an unread field is found when that field is
read and by `cassie verify`). Authority conversion still validates every field.
The caller declares complete-source versus visited-row traversal before opening
its private cursor. An unrestricted SQL source validates the ownership of its
read fields before returning its first rows, and authority conversion validates
complete field ownership before returning its first rows. A bounded source
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
from selection. Since #1358 (owner decision D1) a complete-source traversal
validates only the fields it reads plus the markers: the document cursor runs
its ownership preflight over the selected fields' key ranges, and the vector
reader finds an orphan in a read field while it merges (see "Vector execution"
below). An orphan in an unread field is no longer seen by that query; it is
found when the field is read and by `cassie verify`. Before #1358 the preflight
scanned every non-retired field and probed the marker of each field key, about
1.6M extra reads per query at 100k rows. The preflight never decodes a payload.
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

### Compression and pruning (#1357)

Authority stays exactly one thing: the field keys. Two mechanisms make the
column layout cheaper to store and to scan without adding a second source of
truth.

**Compressed field values.** A field value is `[type tag][payload]`. When a
compact value is at least 128 bytes and LZ4 saves at least a quarter of its
bytes, it is stored as `[0x80][original length: u32 BE][LZ4 block]` and
expanded on every read before the usual declared-type validation. Smaller or
incompressible values are stored unchanged, so incompressible data pays no
space and the only added write cost is one bounded compression attempt per
value of at least 128 bytes. A declared length outside 128 bytes to 64 MiB, a
block that does not expand to exactly its declared length, or a tag-0x80 value
that fails the type check is `XX001` corruption. Query-memory accounting and
conversion charge the expanded length, not the stored length, before expanding.
Numeric and short-string columns are not shrunk by this mechanism: each field
key carries the relation, field id and row id, which dominates a short value.
Block-packed segments (one encoded chunk per field per segment) would change
the authority layout and are out of scope here.

**Segment summaries (rebuildable, derived).** A summary record covers one
contiguous row-id range of up to 512 live rows (built in segments of 256; a
range that grows past 512 is cut again into 256-row pieces). The record stores
the schema version it was built at, the live-marker count, a SHA-256
fingerprint over every marker key and value in the range, and for each
summarizable column (`SMALLINT`/`INT`/`BIGINT`, `TEXT`/`VARCHAR` up to 256
bytes, `BOOLEAN`) its non-NULL count, minimum and maximum. A column with a
missing key (a field added after the row), a longer text value or any other
type is left unsummarized in that segment. Floats, `CHAR(n)`, temporal,
composite, domain and every other type are never summarized. The record ends in
its own SHA-256 checksum; the first four bytes are `CAS1` and a `u16` format
version.

*Using them.* A scan whose filter is a conjunction of `=`, `<`, `<=`, `>`, `>=`,
`BETWEEN`, `IS NULL` or `IS NOT NULL` on summarized columns may skip a segment.
At the first live marker of a segment whose bounds exclude any conjunct, the
reader re-reads only that segment's markers and compares their count and
fingerprint with the record. Only an exact match skips it: no field payload is
fetched or decoded for the segment, and the scan resumes after its last marker.
Literals must be directly comparable with the column (an integer or float
literal for an integer column, a string for text, a boolean for boolean);
anything else prunes nothing. Used by the typed projected read, the ungrouped
typed aggregate and the controlled projected fallback; the filter is always
applied again to every row that is read.

*Why a summary cannot hide a match.* Every write bumps the row's marker
revision in the same transaction as its field keys, so the fingerprint changes
whenever any row of the range is inserted, updated or deleted. A reader bypasses
(reads the segment normally and counts `summaries_rejected` or `summaries_stale`)
a record that is missing, fails its checksum or key binding, has another format
version or schema version, or whose fingerprint does not match. A bypass is
never an error: the query returns the complete authoritative result. A forged
record with a valid checksum and fingerprint is out of scope: summaries are
derived metadata under the same trust boundary as the field keys.

*Maintenance and publication.* After a statement commits, a separate
transaction rebuilds only the segments that contain a changed row, or all
segments when no record exists, a record is unusable, the schema version
changed, or a scan reported stale records. The rebuild reads one snapshot, so a
record's fingerprint and bounds describe the same rows. A crash or failure
between the write and the refresh therefore leaves stale records, never wrong
ones, and the next refresh repairs them. Interrupted publication cannot expose
a half-written summary because each record is one key written atomically.
Drop, conversion and rename already remove or move the whole `[rel, ...]`
family range, which includes summaries. Cost: a refresh reads at most two
segments (512 rows) per touched segment, so a one-row commit costs up to
about 512 marker reads plus 512 field reads per summarized column.

*Version admission.* The layout stays `cassie-midge-layout-v4-0.5.0-columnar`:
that layout is unreleased, so the `s` key kind and the `0x80` value tag are
part of its definition and no earlier-format store needs migrating. A build
without them reads a compressed value as a type-tag mismatch (`XX001`) and a
summary key as an unknown discriminator it never reads. Within the summary
format, an unknown `CAS1` version is bypassed and rebuilt, so the format can
change without a layout raise.

*Corrupt authority.* Pruning never replaces validation. The document cursor of
a complete-source scan validates field-to-marker ownership of the fields it
reads for the whole table before its first row, so an orphan in a read field of
a skipped segment still fails with `XX001` there. The vector reader never opens
a skipped segment, so it does not see an orphan inside one (nor a missing or
undecodable value, exactly as for unselected fields and unvisited rows under
#1356); `cassie verify` and the storage verifier report them.

*Counters.* `Midge::column_pruning_for_diagnostics()` returns
`segments_skipped`, `rows_skipped`, `summaries_loaded`, `summaries_rejected`,
`summaries_stale` and `fingerprint_markers` (markers re-read to verify skip
candidates; never payloads). A skipped segment adds nothing to
`column_field_reads_for_diagnostics()` and does not count as a visited marker.
`set_column_pruning_enabled_for_diagnostics(false)` forces unpruned reads for
differential tests.

*Measured on the pinned fixtures* (`tests/storage_indexes/column_store/pruning*.rs`,
1,280 rows, summaries rebuilt to segments of 256, 256, 256 and 512 rows):

| Fixture | Result |
| --- | --- |
| Sorted `amount >= 1000`, two selected columns | 3 of 4 segments skipped, 768 rows; 1,024 payload reads and decodes instead of 2,560 |
| Sorted `tenant = 'tenant-04'` | 3 segments skipped; reads 1,024 instead of 2,560 |
| `note IS NULL` (NULLs only in the last 180 rows) | 3 segments skipped |
| Shuffled values, or `amount >= 0` | 0 segments skipped; every row read |
| Compressible text (`"event-log-line " x 24` plus a row tag, 1,280 rows) | field bytes 554,240 before, 128,000 after; summaries 752 bytes; selective read fetches 23,552 payload bytes instead of 58,880 |
| Incompressible text (375 pseudo-random letters, 1,280 rows) | stored bytes equal the uncompressed encoding exactly; no value compressed |

### Vector execution (#1358)

An eligible query over a column-authoritative table reads *vector batches*: the
typed scan source decodes stored field values straight into typed lanes and
never builds a row document. The projected typed read, the ungrouped typed
aggregates and the typed grouped hash aggregate all gain the source through
`TypedScanStream`; nothing above it changes.

**Reader.** `Midge::open_column_vector_reader` returns batches of at most 1,024
rows (owner decision D8), in row-id order, that never cross a segment boundary
(one id range per segment when pruning plans ranges, one whole-table range
otherwise). Per batch it reads up to 1,024 live markers and opens one bounded
Midge range iterator per selected field over the same id span, then walks them
in id order:

| Field entry versus marker | Result |
| --- | --- |
| id sorts before the marker | orphan: `XX001` on a complete-source scan, skipped on a visited-row scan (D6) |
| id equals the marker | the value |
| id sorts after the marker, or the iterator is exhausted | `XX001` when the field is required at the marker's schema version, otherwise NULL (the field postdates the row) |
| entries left after the last marker | orphans |

A stored value is expanded (compression) and tag-checked exactly like
`decode_compact_field_value`: integers (width-checked) into `i64` lanes, floats
(a non-finite value is rejected as before) into `f64` lanes, Booleans, and text
into UTF-8 lanes without a JSON value; every other supported type passes
through the existing decoder and JSON-to-value conversion into a generic value
lane, one value per cell. Ownership validation is exactly the fields read plus
the markers (D1). The vector path adds nothing to
`column_validation_reads_for_diagnostics()`.

**Eligibility** (one function for EXPLAIN and dispatch, `column_vector_eligibility`).
The scan reads vector batches when the table is column-authoritative, every
field the typed operator needs is a plain stored field of a supported type, and
the operator accepts the plan shape. Supported types: `SMALLINT`, `INT`,
`BIGINT`, `FLOAT`, `BOOLEAN`, `TEXT`, `VARCHAR`, `CHAR` (native lanes) and
`NAME`, `UUID`, `DATE`, `TIME`, `TIMESTAMP` and the extended scalar types
(generic lanes). The shapes are the existing typed ones: `COUNT`, `SUM`, `AVG`,
`MIN`, `MAX` (and the Boolean aggregates) over numeric columns with a native
filter, ungrouped or grouped by plain columns, plus the typed projected read
(no `ORDER BY`, `DISTINCT`, window or set operation). Everything else reads row
documents; `EXPLAIN` says why:

| `column_vector_reason` | Row documents because |
| --- | --- |
| `column-vector-unsupported-type:<type>` | a needed field is document-shaped (JSON, array, composite, domain, enum, vector, bytea) or otherwise unsupported |
| `column-vector-unsupported-expression` | the plan holds a function, sub-select, `ORDER BY`, `DISTINCT` or filter expression the typed read cannot evaluate |
| `column-vector-unsupported-aggregate-shape` | the aggregate carries `DISTINCT`, `FILTER`, ordering, an unsupported function or argument, or a grouping key of an unsupported type |
| `column-vector-unsupported-transport-budget` | the query budget cannot hold one typed carrier (1,024 values) |
| `column-vector-unsupported-join` | the source is a join |
| `column-vector-covering-column-index` | a covering column index serves the filtered or limited read |

`column_vector=eligible|fallback|not_applicable` and `column_vector_reason=`
appear on every plan; `EXPLAIN ANALYZE` adds `column_vector_batches_delta`,
`column_vector_lanes_delta`, `column_vector_values_delta`,
`column_vector_staged_rows_delta`, `column_vector_segments_pruned_delta` and
`row_documents_delta`. An eligible statement reports `row_documents_delta=0`.

**Open transactions (D5).** A scan merges the session's staged rows serially in
id order with the persisted batches: an upsert replaces a persisted lane or
inserts one, a delete drops it. A persisted batch no staged id falls inside is
passed through as lanes; a batch with a staged row inside is merged value by
value. `REPEATABLE READ` uses the statement snapshot exactly like the document
cursor.

**Pruning.** When the plan has a pruning filter and the table has summaries,
the reader reads only segments whose summary does not exclude the filter. A
segment is skipped only when its summary is trusted (schema version and marker
fingerprint match, the #1357 check); an untrusted summary leaves the segment in
scope. A segment that holds an id the open transaction staged is never skipped.
Skipped segments are never opened by a field iterator, so none of their values
is fetched or decoded.

**Memory and cancellation.** A batch reserves query memory for its markers, the
iterator keys, the lanes (per lane carrier) and each decoded value's expansion
before decoding it; cancellation and the deadline are checked per batch, per
marker and per value, and no partial batch is returned. The reservation is
released once the typed columns retain the lanes.

**Counters** (`Midge::column_vector_reads_for_diagnostics()`, difference with
`.since()`): `scans`, `batches`, `lanes` (rows times selected fields),
`segments_pruned`, `segments_untrusted`, `values_fetched`, `values_decoded`,
`row_documents_built` (also incremented by the document cursor and the batched
column scan; point loads by id are not counted) and `staged_rows_merged`. The
selected-field counters of #1356 (`column_field_reads_for_diagnostics()`) also
count the vector reader's fetches and decodes.
`set_column_vector_enabled_for_diagnostics(false)` forces row documents for
differential tests and benchmarks.

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
