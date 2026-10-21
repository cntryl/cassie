# Storage lifecycle and recovery qualification

This record defines the finite single-node Cassie obligations selected by
[#771](https://github.com/cntryl/cassie/issues/771), after prerequisite #751.
The qualification baseline is `3b03dd4db34ac88f533501d0484ecdc3cf223a2e`.
The current Cassie layout marker is `cassie-midge-layout-v3`; each database's
catalog records now share its row family. This unreleased layout does not need
an in-place migration. Image framing, journals and the direct Midge API remain
authoritative at their documented boundaries.
Named witnesses qualify their exercised boundaries, rather than every possible
interleaving or deployment profile. Full ordered validation and exact-head hosted
checks remain publication requirements.

| Obligation | Qualification owner and finite disposition |
| --- | --- |
| STO-01 | `midge_baseline_database_families` and catalog duplicate-name restart witnesses exercise stable opaque owners; each database's rows and catalog share its family, while `transactions.rs` keeps transactions within one physical family. |
| STO-02 | `midge_layout_bootstrap` plus `layout_marker_admission` reject old, corrupt and missing markers before bootstrap mutation. Initialized empty and populated storage retain exact family contents on rejection, including absent `cf1`. Only empty default/fixed `cf0`/`cf1` inventories qualify as unpublished initialization; unknown or opaque families, even empty, do not. |
| STO-03 | `schema_operation_recovery` and `index_publication_recovery` exercise journal replay, rejected intent and interrupted publication; recovery preserves newer owners rather than silently resetting storage. |
| STO-04 | `column_name_reuse_resolution` exercises rename/drop/re-add field identities and declared carriers; schema recovery witnesses cover interrupted field maintenance. |
| STO-05 | `schema_epoch_safety` exercises retained schema snapshots and deferred cleanup. The separate query-plan cache semantics version remains its own binding compatibility fence. |
| STO-06 | `storage_integrity` exercises failed commit atomicity. `documents/commit.rs` commits rows, generation, maintenance debt and data epoch in the same data transaction. |
| STO-07 | `index_publication_recovery` exercises prepared scalar, column and vector publication visibility and replay. |
| STO-08 | `documents/commit.rs` retry-policy units distinguish conflict, stall, fenced writer and attempt exhaustion. Prepared sourced-vector replay witnesses count provider calls; providers are not rerun by storage commit retries. |
| REC-01 | Column missing-segment restart and domain missing-manifest restart witnesses exercise complete fallback followed by rebuild. Partial derived results are not authoritative rows. |
| REC-02 | `vector_write_path` exercises provider/constraint failure, prepared-intent replay, version/digest/staging validation and source-generation fencing. The prepared payload, rather than a new provider request, owns replay. |
| REC-03 | `analytical_projection_recovery` and expression-rollup restart witnesses exercise durable debt after a committed source write. |
| REC-04 | `database_image_generation` exercises atomic source mutation across pages, mutation before the first chunk, DDL after catalog capture, physical-family replacement and unchanged successful export. Snapshot mutation and manifest generation witnesses remain the filesystem-image owners. |
| REC-05 | Database-image framing probes reject bad magic/version/count/hash, truncated or reordered frames, missing header, extra footer bytes and invalid lengths. Rejected prefixes never publish. Abort flushes uniquely owned disposable staging before Midge safe-drop and retains session ownership if cleanup errors. A database published while upload is staged must survive a rejected restore. Creation finalization also reads target absence in its publication transaction, so a creator journaled before a successful restore cannot overwrite that restore. |
| REC-06 | `snapshot_restore` exercises nested paths, symlink aliases, invalid manifests and partial-copy cleanup while preserving the source. |
| REC-07 | Projection replay witnesses exercise duplicate/concurrent/out-of-order events, batch duplicate IDs and monotonic checkpoints. Replay and checkpoint ownership remain in `app/replay.rs`. |
| REC-08 | `storage_config.rs` selects Midge `Local` sync/buffered, `CloudBackground` async and `CloudStrict` strict policies; acknowledged adapter transactions use the selected commit policy. Local restart witnesses qualify adapter-visible integration. Midge WAL, lease and manifest internals remain external. |

The mixed-generation streaming defect was reproduced with 300 rows and one
atomic update: the old implementation restored 256 old and 44 new values despite
valid count/hash framing. The durable record is
[#771's confirmed-defect comment](https://github.com/cntryl/cassie/issues/771#issuecomment-6039621934).
The fix rejects a changed captured schema epoch, database data epoch or physical
owner before chunk admission and footer creation, including a check after catalog
capture. A partial chunk is not a completed backup; callers must discard rejected
streams. A finished coherent image remains usable after later source changes.

The same qualification found unflushed staging could prevent abort and that
missing markers were silently recreated on initialized storage. Safe cleanup now
flushes before Midge safe-drop, without force-discard or treating `Busy` as a
license to destroy a family. Marker validation runs before fixed-family creation,
registry replay or data admission. These are existing-contract corrections,
not migrations or new persistent formats.

External detached signature verification and expected signer identity remain the
operator-owned procedure in [snapshot restore](snapshot-restore.md). Local
checksums do not authenticate provenance. Cloud profiles, Linux native readiness,
operational soak drills and authenticated deployment qualification are not
promoted by this finite local record. Failed-cleanup retry retention is a source
law; successful prefix cleanup and repeated abort have executable witnesses,
but no fabricated runtime fault-injection proof is claimed.

## Midge 0.4.1 fresh-store boundary (2026-10-17)

Cassie selects the published `cntryl-midge` 0.4.1 release for fresh stores.
Midge requires database FORMAT 8, SST V4, cloud WAL catalog v3 and cloud lease
metadata v3. Stores created with the former 0.3.2 dependency are not compatible;
there is no in-place upgrade. As of this 2026-10-17 snapshot, Cassie's own
logical layout marker was unchanged.

For a fresh deployment, select a new empty `CASSIE_STORAGE_PATH` (or a fresh
cloud prefix) and retain the old store separately. Do not delete the old store
or copy its WAL, catalog, lease or metadata files into the new store. Existing
data requires logical export with the old binary and import into a new store,
followed by application-level verification before switching traffic. This
checkpoint adopts fresh stores only; it does not implement or qualify a
migration utility, migrate an existing deployment, or qualify a cloud provider.
See the [Midge 0.4.1 migration guide](https://github.com/cntryl/midge/blob/v0.4.1/docs/operations/migration-guide.md#032-to-040).

Midge's `OutcomeUnknown` means a write may already be committed despite failed
acknowledgment. Cassie returns a terminal storage error with reconciliation
guidance. It must not become an automatic write retry because its message
contains “write conflict”. Only the typed `WriteConflict` variant licenses that
retry; corruption and other terminal variants remain terminal regardless of
message text. The existing bounded conflict/stall retry policies are retained.

Validation evidence and remaining compatibility failures are recorded in
[validation](testing/validation.md).
