# Snapshot And Restore

Cassie snapshot manifest v2 defines local single-node recovery artifacts.
They complement replay and rebuild; they do not provide remote backup orchestration, replication, quorum recovery, or distributed failover.

## Format

A snapshot directory contains:

| Path | Contents |
| --- | --- |
| `cassie-snapshot-manifest.json` | Cassie manifest with compatibility and projection metadata. |
| `midge/` | Recursive copy of the local Midge data directory. |

The copied Midge directory includes `cf0`, `cf1`, and every opaque per-database
`db-*` family registered in the source catalog. The manifest records:

- snapshot manifest format version (currently 2)
- Cassie version
- generated timestamp in milliseconds
- schema and data epochs
- every collection and its current generation
- compatibility status
- Midge data path inside the snapshot
- projection id, kind, collection, schema version, active version, source checkpoint, source position, and hash metadata

Projection hash metadata includes algorithm, digest length, canonical encoder version, row/range/root hash versions, root digest, root state, row count, and range count.

## Workflow

Create snapshots from a local data directory with `Cassie::create_snapshot_from_data_dir`. The v2
manifest records schema/data epochs and every collection generation before copying, then verifies
the same values after the copy. If the source changed during the copy, Cassie removes the partial
payload and returns an error so the caller can retry.
Restore snapshots into an empty local data directory with `Cassie::restore_snapshot`, then start a Cassie instance against that restored directory.

Restore validates the manifest before copying data and validates the copied Midge state before accepting the target.
It rejects v1 and every other non-v2 snapshot manifest; Cassie does not provide a legacy snapshot reader or migration path. Restore also rejects incompatible Cassie versions, non-compatible status, unsupported Midge data paths, invalid projection hash metadata, epoch or collection-generation mismatches, projection-state mismatches, and malformed recovery journal/debt records.

## Safety Boundary

Snapshot creation closes its metadata reader before copying the Midge directory so recovered local state is durable on disk before the filesystem copy. Failed snapshot copies remove the partial snapshot directory, and failed restores remove the partial target directory; callers can retry from the original source or snapshot.
Restored instances hydrate normal catalog, projection, and query state through the regular startup path.

External tooling remains responsible for:

- scheduling snapshot creation
- stopping or quiescing write traffic before snapshot
- moving snapshots off-node
- encryption, retention, and access control
- verifying remote object-store integrity
- choosing failover and routing policy

Manifest and database-image checksums provide integrity checks, not producer
authentication. Cassie does not sign recovery artifacts; use authenticated
transport or external signing when an artifact crosses an untrusted channel.

## External Signing Runbook

Use the organization's approved detached-signature tooling and identity policy; Cassie does
not define a signing envelope, key format, trust store, or certificate profile.

1. Receive the database image and detached signature as separate files over an authenticated
   channel.
2. Resolve the expected signer identity and trust root from deployment policy, not from
   metadata bundled beside the image.
3. Verify the detached signature over the exact image bytes and require the verifier to match
   the trusted identity. Stop on an invalid signature, unknown identity, expired trust
   material, verification warning, or changed image.
4. Record the verifier, trusted identity, image digest, verification time, and release/change
   reference in the operator audit log.
5. Only after verification succeeds, stream the unchanged image bytes into
   `RESTORE DATABASE ... FROM STDIN`.
6. Retain the signature and verification record with the restore evidence.

Cassie's image checksum is still checked during `RESTORE`; that proves transfer and framing
integrity, while the detached signature and trusted identity establish provenance.

Snapshots do not change query planning and do not add distributed execution, cross-node reads, replication, consensus, or automatic repair.

For release-architecture rehearsal, use the [Cross-Architecture Release Rehearsal](cross-architecture-rehearsal.md)
record and verify restore-to-new-directory results separately for amd64 and arm64.
