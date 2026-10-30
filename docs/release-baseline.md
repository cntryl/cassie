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
