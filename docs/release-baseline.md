# Cassie 0.1.0 Release Baseline

Cassie has never been released. The package version in `Cargo.toml` is reset to
`0.1.0`; the earlier `0.2.0` value and the former v0.3 Projection Store/v0.4
Query roadmap labels were unreleased development planning. Cassie 0.1.0 is the
actual starting baseline for the selected MVP. It is a planned release
baseline, not evidence that a release has already been published or qualified.
The repository had no local/remote tags or GitHub releases on 2026-10-30.

Local qualification recorded before this reset applies to its recorded
candidate tree only. It does not qualify the new 0.1.0 package and format
identifiers; the exact release commit still needs its own ordered gate.

The [Product Roadmap](product-roadmap.md) owns the MVP scope and release order.
The package version remains the release version authority. The next feature
releases are v0.2.0 Query and Retrieval and v0.3.0 Authoritative Columnar
Tables. Production readiness remains a separate qualification gate, not a
feature milestone.

## Cassie-owned persistent formats

Cassie-owned durable format identifiers start at version 1 for this baseline:

- row blobs use `CRB1`;
- column-batch manifests, row-ID chunks, and field chunks use `CBM1`, `CBR1`,
  and `CBC1`;
- graph adjacency, full-text state, vector-index state, HNSW records, rollup
  definitions, time-series indexes, projection manifests, verification hashes,
  and snapshots use format version 1;
- database images use format version 1;
- the fresh Cassie storage epoch is identified by
  `cassie-midge-layout-v1-0.1.0`.

This reset changes Cassie-owned identifiers only. Dependency versions, Midge's
database and SST formats, and the PostgreSQL wire protocol version retain their
own owners and numbering.

## Existing pre-release data

Pre-release Cassie storage and snapshots are not compatibility inputs for
0.1.0. Cassie rejects stores with prior layout markers before modifying them,
and snapshot restore rejects the former version-2 manifest. Create a fresh
storage directory and new snapshots for 0.1.0; there is no in-place migration
from unreleased development formats.


## Selected v0.2 compatibility transition (2026-11-04)

The user approved this contract direction on 2026-11-04. It is selected
implementation scope, not implemented or qualified support. Existing rejection
and compatibility boundaries remain until the owning acceptance cases pass.

v0.2 reads existing v1 stores without rewriting rows. Before the first write using
persisted null-safe definitions, named types, object grants, extensions, or comments,
advance the store compatibility marker to the explicit cassie-midge-layout-v2-0.2.0
identifier. v0.1 must then reject that store before mutation. The monotonic marker
change precedes feature publication; a crash between the marker and feature transaction
leaves a valid v0.2-readable store with no partial feature. This is not a cross-family
transaction. New database-scoped feature records and their row/catalog effects publish
together in the owning Midge database family. Add versioned feature records, immutable
object IDs and minimum-reader requirements; reject unknown versions before publication.
Database images/snapshots retain the marker, all referenced descriptors and
authoritative metadata. No automatic row rewrite, destructive recreate, second storage
layer, or migration from older pre-v0.1 development layouts is selected.

The selected new marker is `cassie-midge-layout-v2-0.2.0`, stored at
the existing schema-family marker key. Existing key namespaces remain unchanged.
This permits a one-way storage compatibility transition for stores that use new durable
features. Staying on v1 remains possible until first feature use. A user needing
downgrade after feature use needs a separate export/conversion contract.

This is not a package-version or dependency-version change. Prove marker/crash
ordering, unchanged v1-row reads, v1-reader rejection before mutation, rollback,
restart and image/restore admission before publishing new durable features.
