# Storage compatibility transition

This is the implementation plan for the already selected
[v0.2 compatibility transition](release-baseline.md#selected-v02-compatibility-transition-2026-11-04).
It is a prerequisite for persisted null-safe definitions (#798), named types
(#828), grants (#831), extensions (#843) and comments (#845). Those features
remain independently pending. This plan does not change Midge 0.5.1, row
encoding, dependency versions or release configuration.

The existing schema-family marker remains the authority. A fresh store starts
at `cassie-midge-layout-v1-0.1.0`. A v0.2 reader recognizes that exact marker and
`cassie-midge-layout-v2-0.2.0`; unknown and missing initialized-store markers
fail admission before Cassie creates or publishes catalog objects. Merely
opening a v1 store does not advance it or rewrite its rows.

Before publishing a feature that requires v0.2, a private Cassie adapter gate
serializes the marker transition. It reads the marker in a fresh schema-family
transaction and acknowledges its monotonic requirement before the feature's
database-family transaction can publish. Any marker error stops that operation.
An acknowledged marker followed by an aborted feature remains a valid upgraded
store. Requiring v1 in a v2 store leaves the v2 marker intact. Each feature owner
must additionally validate its own versioned definition records on admission,
load and restore; recognizing the store marker does not validate a feature.

Every v2 requirement reasserts the same v2 bytes in one strong commit, even when
the marker is already locally visible. Published Midge documents that an
`OutcomeUnknown` write may be visible without acknowledged durability, and an
empty cloud transaction does not execute the upload finalizer. Jev identified
this early-return gap (0.91) and selected a fresh identical marker assertion
(0.76). A focused native-sequence probe must observe an actual new write for an
existing v2 requirement. The helper does not retry errors or feature publication.

The compatibility anchor uses local `WriteOptions::sync()` and cloud
`WriteOptions::cloud_strict()`, including background cloud configurations.
Published Midge 0.5.1 documents that local Sync waits for fsync, CloudStrict
waits for upload, and CloudAsync acknowledges local visibility with background
upload. Normal data-write policies retain their existing configuration. Jev
selected this anchor-only policy (0.71); the deferred gate must verify its
actual call site and the available local/cloud controls.

Version 1 database images remain readable only with a v1 layout requirement;
their legacy checksum covers catalog and data entries. A v1-layout backup
continues to use version 1. A v2-layout backup uses image version 2, whose
checksum also covers the exact encoded header, including the layout marker.
The existing public `DATABASE_IMAGE_VERSION` constant remains 1 and identifies
the legacy image version; it is not a universal version for every emitted
backup. `DATABASE_IMAGE_VERSION_WITH_PROTECTED_HEADER` identifies version 2.
The emitted version depends on the store layout requirement.
Readers reject other image-version/layout combinations. This detects accidental
header changes, including downgrade of both version and marker, but SHA-256 is
not authentication against intentional image rewriting. A backup captures
the actual store marker with its source generations and includes it in the
header. A marker transition invalidates an active stream before its next
chunk, like an existing source-generation change. A v2 store conservatively
exports v2 images even for a database without new features. Jev selected this
law (1.00); no per-database minimum-reader metadata is assumed.

Restore accepts only the known image/layout versions. It checks the complete
footer and integrity before requiring the header's marker, then publishes the
staged database. A malformed, incomplete or aborted image cannot advance a v1
store. If the marker commits and later publication fails, the target stays
unpublished and the marker remains v2. A v1 image cannot downgrade a v2 store.

Authored probes will cover unchanged v1 rows across upgrade/restart, the gap
between marker acknowledgment and feature publication, concurrent idempotent
upgrade, unsupported/missing markers, conservative backup headers, stream
invalidation, restore integrity/order and no downgrade. They remain unexecuted
until the next coherent ordered gate. Source and Jev evidence is retained in
the ignored `target/v020-evidence/storage-compatibility/` directory.
