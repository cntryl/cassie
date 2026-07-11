# Per-Database Midge Column Families

Cassie storage layout v5 uses one physical Midge column family for each logical
database:

| Family | Ownership |
| --- | --- |
| `cf0` | Global schema/catalog records and the database registry |
| `cf1` | Cassie temporary and transient runtime state |
| `db-*` | One opaque, stable family for one logical database's data plane |
| Midge `default` | Reserved compatibility family; Cassie does not use it |

The `db-*` name is generated once and persisted in `DatabaseMeta`. A logical
database name is not encoded in the physical family name, so future logical
renames do not require rewriting data. The registry and lifecycle journal live
in `cf0`.

All rows and database-owned derived state use the database family: scalar,
time-series, vector, full-text, column-batch, column-store, graph, hash, and
maintenance records. Their keys contain the local schema/relation scope; the
database component remains in catalog keys in `cf0`. `Midge::database_tx` is
the routing boundary, and a transaction cannot span databases or mix catalog
and data families.

Database creation journals the intended family before creating it and commits
the catalog record only after the family exists. Empty-only database drops use
the same journal so startup can finish an interrupted drop or remove an
orphaned create/restore family. A registry/family mismatch is a startup error.
There is no online migration: v4 and older stores must be recreated for v5.

## Logical database images

`Cassie::begin_database_backup` emits a bounded sequence of length-delimited
frames containing a versioned header, database-scoped catalog entries, raw
entries from exactly one database family, and a checksummed footer. Global
roles, routines, server metadata, temporary state, caches, and transient
reports are excluded. `Cassie::begin_database_restore` stages an opaque family
and keeps it invisible until all frames, catalog rewrites, counts, and checksums
commit. The target database must not already exist.

Pgwire exposes the same logical image stream through simple queries:

```sql
BACKUP DATABASE analytics TO STDOUT;
RESTORE DATABASE restored FROM STDIN;
```

These commands use `CopyOut`/streaming `CopyIn`, are administrative operations,
and are rejected inside explicit transactions and unsupported extended-query
paths. Whole-server snapshots remain filesystem copies and therefore include
the dynamic database families together with `cf0` and `cf1`.
