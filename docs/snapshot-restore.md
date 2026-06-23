# Snapshot And Restore

Cassie v1 snapshots are local single-node recovery artifacts.
They complement replay and rebuild; they do not provide remote backup orchestration, replication, quorum recovery, or distributed failover.

## Format

A snapshot directory contains:

| Path | Contents |
| --- | --- |
| `cassie-snapshot-manifest.json` | Cassie manifest with compatibility and projection metadata. |
| `midge/` | Recursive copy of the local Midge data directory. |

The manifest records:

- snapshot format version
- Cassie version
- generated timestamp in milliseconds
- schema epoch
- compatibility status
- Midge data path inside the snapshot
- projection id, kind, collection, schema version, active version, source checkpoint, source position, and hash metadata

Projection hash metadata includes algorithm, digest length, canonical encoder version, row/range/root hash versions, root digest, root state, row count, and range count.

## Workflow

Create snapshots from a quiesced local data directory with `Cassie::create_snapshot_from_data_dir`.
Restore snapshots into an empty local data directory with `Cassie::restore_snapshot`, then start a Cassie instance against that restored directory.

Restore validates the manifest before copying data.
It rejects unsupported snapshot format versions, incompatible Cassie versions, non-compatible status, unsupported Midge data paths, and invalid projection hash metadata.

## Safety Boundary

Snapshot creation closes its metadata reader before copying the Midge directory so recovered local state is durable on disk before the filesystem copy.
Restored instances hydrate normal catalog, projection, and query state through the regular startup path.

External tooling remains responsible for:

- scheduling snapshot creation
- stopping or quiescing write traffic before snapshot
- moving snapshots off-node
- encryption, retention, and access control
- verifying remote object-store integrity
- choosing failover and routing policy

Snapshots do not change query planning and do not add distributed execution, cross-node reads, replication, consensus, or automatic repair.
