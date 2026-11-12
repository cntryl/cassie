# SQL object comments — issue #845

## Selected v0.2 contract

Support `COMMENT ON SCHEMA`, `COMMENT ON TABLE`, and `COMMENT ON COLUMN` for
persisted schemas and physical tables in the current database. Column comments
use the persisted Midge `RowSchema.field_id` as `pg_attribute.attnum`; the field
identity remains stable across rename and retired IDs are never reused. SQL
views, materialized views, indexes, routines, types, databases, and other object
forms are outside this finite contract and must fail deterministically. This is
the active v0.2 acceptance in the current issue body; the earlier routine/type
list retained in its historical details is not part of this slice.

`COMMENT ... IS NULL` and `COMMENT ... IS ''` both remove a record. This follows
PostgreSQL 18's COMMENT semantics, which were more specific than the previous
Cassie note that treated an empty string as a retained comment. A stored comment
is returned as text with OID 25; an absent comment is SQL NULL.

The parser accepts a string literal or SQL `NULL` after `IS`; expressions and
bound parameters are deterministically unsupported because COMMENT is a utility
statement with a literal value in the selected contract. `obj_description`
supports its one-argument form and its `(oid, name)` form for `pg_class` and
`pg_namespace`; `col_description` accepts `(oid, int4)` and a positive stable
attribute number. Both functions return nullable text.

Persist each comment in a versioned record in the active Midge database family,
keyed by `(classoid, objoid, objsubid)`. Schema addresses use the stable schema
OID and `pg_namespace` class OID 2615 with sub-ID zero. Table and column
addresses use the stable relation OID and `pg_class` class OID 1259; table
comments use sub-ID zero and column comments use the stable attribute number.
Database-family placement scopes identical OIDs in separate databases.
Records carry the current format version and exact text; malformed versions or
payloads fail reads instead of silently hiding comments. No migration or
backward compatibility between unreleased builds is required.

Write, clear, rename, and drop cleanup through the same database-family
transaction overlay as the owning catalog operation. Savepoint and transaction
rollback discard staged comment changes; commit publishes them. Stable object
addresses preserve comments through table/schema/column renames. Dropping a
schema or table removes its comment records, and dropping a column removes the
record for its retired attribute. Current-build local restart and database
image restore retain the exact comments and object ownership metadata.

The current role contract gives all selected schema/table owners to `root` and
rejects DDL from authenticated non-root roles. `COMMENT` therefore follows the
same root authorization boundary; adding non-root object ownership or grants is
owned by the separate #831 access contract. Authorization must be checked at
statement admission and again when a suspended execution resumes.

`obj_description` returns comments for the selected schema/table OIDs, with
the two-argument form limited to `pg_namespace` and `pg_class`. The one-argument
form searches the selected object classes by stable OID. `col_description`
uses table OID and stable positive attribute number. `pg_description` exposes
only the finite fields `objoid`, `classoid`, `objsubid`, and `description`,
with one row per present selected comment. All three reads are database scoped.

## Decision evidence

Jev selected versioned stable-address records over embedding comment strings in
name-keyed metadata (0.91 probability, 0.86 confidence). It selected empty
string as removal (0.71 probability, 0.42 confidence) and physical tables only
for the relation boundary (0.87 probability, 0.80 confidence). These are
contract leads; source, PostgreSQL semantics, and authored regressions remain
the correctness evidence. The key-free request and response are retained under
`target/v020-evidence/845/`.

PostgreSQL 18 documents empty-string and NULL removal, object ownership, and
automatic cleanup on drop in [`COMMENT`](https://www.postgresql.org/docs/18/sql-comment.html).
The selected address fields follow [`pg_description`](https://www.postgresql.org/docs/18/catalog-pg-description.html),
and text/NULL function behavior follows the
[`comment information functions`](https://www.postgresql.org/docs/18/functions-info.html).

The stale Feature Support note that previously listed materialized-view
comments as pending under #845 is superseded by this live issue boundary:
`COMMENT ON MATERIALIZED VIEW` remains deterministically unsupported and is
outside #845.

Focused regressions are authored in
[`comment_contract.rs`](../tests/catalog_security/catalogs/descriptions/comment_contract.rs)
and remain unexecuted pending the coherent bundle gate. This record does not
claim runtime support or qualified restart/image behavior until that gate and
its final-head review complete.

An adversarial Jev implementation review assigned a 0.55 probability to an
acceptance gap, above the repository's 0.30 focused-probe threshold. The source
review isolated the finite `col_description(oid, int4)` boundary: an integer
outside int4 range must fail with `integer out of range`, while SQL NULL remains
NULL. A focused unit regression now covers both cases in
`src/executor/filter/comment_functions.rs`. These probes are authored but
unexecuted; this finding is not treated as resolved by the model judgment.
Redacted review evidence is retained under
`target/v020-evidence/845/implementation-review.*.redacted.json`.

A lifecycle-specific Jev follow-up separated transaction atomicity, rename/drop
cleanup, and restart/database-family visibility into three concrete Noul
questions. Jev 1.14 returned 0.10, 0.17, and 0.26 respectively, all below the
0.30 focused-probe threshold. The source already stages comment key writes in
the active Midge DDL overlay and the authored tests cover savepoint/transaction
rollback, rename/drop cleanup, database isolation, and local restart. No
additional model-directed source change was selected. These regressions remain
unexecuted; the probabilities do not establish runtime correctness. Redacted
follow-up evidence is retained under
`target/v020-evidence/845/lifecycle-followup.*.redacted.json`.
