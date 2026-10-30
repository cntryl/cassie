# Per-Database Midge Column Families

Cassie's `cassie-midge-layout-v1-0.1.0` baseline uses one physical Midge column family for each logical
database:

| Family | Ownership |
| --- | --- |
| `cf0` | Database registry and lifecycle records, roles, and global catalog/operational records |
| `cf1` | Cassie temporary and transient runtime state |
| `db-*` | One opaque, stable family for one logical database's rows, SQL catalog, and database-scoped recovery state |
| Midge `default` | Reserved compatibility family; Cassie does not use it |

The `db-*` name is generated once and persisted in `DatabaseMeta`. A logical
database name is not encoded in the physical family name, so future logical
renames do not require rewriting data. The database registry and lifecycle
journal live in `cf0`; each database's table, index, constraint, sequence, view,
routine, namespace, projection, deferred schema cleanup, and related recovery
metadata live with its rows.

Rows and database-owned derived state use the database family: scalar,
time-series, vector, full-text, column-batch, column-store, graph, hash, and
maintenance records. Relation catalog keys remain database-qualified inside
their owning family. `Midge::database_tx` is the routing boundary. A Midge
transaction can now commit one database's rows and catalog records together;
it still cannot span databases or combine a database family with global `cf0`
records.

Database creation journals the intended family before creating it and commits
the catalog record only after the family exists. Empty-only database drops use
the same journal so startup can finish an interrupted drop or remove an
orphaned create/restore family. A registry/family mismatch is a startup error.
This layout has not shipped. No migration utility is required: every
pre-0.1.0 Cassie directory must be recreated before use. The unique 0.1.0
marker prevents older development layouts, including any earlier v1 marker,
from being mistaken for this format. Startup rejects an older marker without
changing the stored data.

## CRB1 array shape encoding

CRB1 array payloads begin with a dimension count followed by each dimension's
length and lower bound, then a row-major element count and tagged values. The
zero-dimensional empty array differs from a SQL NULL field, and a NULL element
differs from a JSON document whose value is null. The decoder validates
dimension arithmetic and requires the shape product to equal the element count.
NULL elements carry a zero-length varint after their null tag.

CRB1 is the 0.1.0 row and array encoding. Existing pre-release stores must be
recreated; no migration utility or legacy row reader is selected. See the
[type contract](type-contract.md#selected-postgresql-array-shape-contract-2026-10-26).

## Logical database images

`Cassie::begin_database_backup` emits a bounded sequence of length-delimited
frames containing a versioned header, database-scoped catalog and recovery
entries, raw database-local entries from exactly one database family, and a
checksummed footer. Global
roles, routines, server metadata, temporary state, caches, and transient
reports are excluded. `Cassie::begin_database_restore` stages an opaque family
and keeps it invisible until all frames, catalog rewrites, counts, and checksums
commit. The target database must not already exist.

The footer checksum proves image integrity, not provenance: it can detect that
the bytes changed, but it cannot prove who created them. Cassie database images
are not signed. Images that cross an untrusted channel require authenticated
transport or an external signing and verification workflow.

Pgwire exposes the same logical image stream through simple queries:

```sql
BACKUP DATABASE analytics TO STDOUT;
RESTORE DATABASE restored FROM STDIN;
```

These commands use `CopyOut`/streaming `CopyIn`, are administrative operations,
and are rejected inside explicit transactions and unsupported extended-query
paths. Whole-server snapshots remain filesystem copies and therefore include
the dynamic database families together with `cf0` and `cf1`.

## Admission and logical image consistency

A `cassie-midge-layout-v1-0.1.0` marker is required before initialized storage is admitted.
A missing or incompatible marker rejects startup without repairing the marker,
creating fixed families or rewriting user rows. A genuinely empty default/fixed
family inventory can finish unpublished initialization; an unknown or opaque
family is not evidence of fresh storage, even when empty.

Logical database image streams fence captured schema/data epochs and physical
family ownership before chunks and footer publication. Concurrent changes reject
the stream; consumers must discard partial bytes. Restore validates complete
framing before publication and rejects a target that became occupied during
upload. Disposable staging is flushed before Midge safe-drop, and abort is
idempotent after cleanup. See [storage qualification](storage-recovery-qualification.md)
for finite witnesses and explicit deployment boundaries.

Database images carry the 0.1.0 layout marker. Catalog and database-qualified
recovery records are rewritten to the target database; database-local row and
sidecar keys remain unchanged. Images carrying an earlier pre-release layout
are rejected and must be recreated from a 0.1.0 store.
