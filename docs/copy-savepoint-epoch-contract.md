# COPY bindings across savepoint rollback

Status: #1286 / PR #1287 merged at `78c6ed77bb600189fa5797d0c58ee28b27e453a7`.
The #1288 contract below is implemented in the local candidate but unqualified;
its regressions have not run. See [the COPY stale-entry contract](copy-stale-entry-contract.md)
and #1279 for the broader effect-admission work.

An active transaction's COPY owner captures the rollback epoch, session identity
and transaction generation under the same session mutex as its descriptor and
storage snapshots. The existing target-binding, pre-predicate entry and guarded
fork checks reject an owner whose epoch no longer matches. A successful or
partially applied rollback-to invalidates previously captured inactive COPY
bindings even when the target and storage contents are unchanged. A fresh owner
captured afterward is usable. Missing savepoints and ordinary pre-mutation
rollback failures preserve the epoch; RELEASE without rollback also preserves it.
Retain the identity, generation, database, descriptor, storage and physical
identity checks: the per-transaction epoch alone cannot distinguish transactions.

Binding first captures a provisional owner to resolve a target, and then a final
owner under catalog and collection gates. Reject a difference in captured identity,
generation or epoch before returning the final binding, even when both captures
resolve the same name. This deliberately rejects some existing concurrent
same-session COPY/rollback interleavings. Each capture is protected by the
session mutex; no new lock is held during execution. Existing target resolution
and cancellation errors encountered before this comparison retain precedence.

Payload decoding and header validation precede the entry owner check, and their
errors retain precedence. Cancellation/deadline checks retain their existing
positions. At entry, a stale owner takes precedence over a failed transaction, while a
current owner in a failed transaction returns 25P02. The existing target-binding
check returns the stale-owner error for a failed transaction even when the owner
is current. The existing guarded fork checks failed status before owner
validation, and so returns 25P02 first when both apply. Stale checks return the existing 40001
serialization failure. Unowned autocommit COPY retains its existing behavior.

This is a stale-at-check guarantee. It does not fence a rollback after the last
check, independently durable effects already in flight, post-unlock rollback
cleanup, or an already-submitted native write. It does not implement the #1279
shared effect owner or change SQL durability. A regression must pair an already
bound same-generation rollback and a provisional-to-final rollback with exact
errors and an identical valid WHERE predicate that actually advances a sequence
and inserts a row. Additional controls cover failure/interrupt ordering, rollback
failures, RELEASE, fresh/new-generation owners and unowned COPY. Authored tests
are not observed failures or passing evidence until run.

Jev selected the epoch check and provisional-to-final rejection (each 1.0), and
existing per-path error precedence (1.0). Risks identified include the remaining
after-check race, earlier parse/interrupt errors and conservative rejection of
concurrent clone interleavings. Source-derived options and advisory receipts are
preserved in the #1279 design worktree under
`target/1279-design/copy-epoch-{request,response}.json` and
`target/1279-design/copy-capture-choice-{request,response}.json`.
