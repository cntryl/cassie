# ReadyForQuery transaction status snapshot

Status: #1291; selected source-backed contract, not implemented or qualified.
This fixes a point-in-time wire-status race and does not implement #1290's
operation-owned failure handling, transaction retirement or #1279 admission.

ReadyForQuery maps one authoritative session transaction status snapshot to
`I` for idle, `T` for an active transaction, and `E` for a failed transaction.
The snapshot is captured under the existing session mutex. Release that mutex
before encoding or awaiting the frame write. A concurrent change after the
snapshot may make the emitted state older than the session at socket delivery;
the frame represents the captured point in time, not a guarantee about later
state. Preserve the existing public `transaction_status()` strings and the
existing sequential wire mapping. A future transaction status must have an
explicitly selected wire mapping rather than silently becoming Idle.

At baseline `12431805adee449eb2775116faadde724c7ae8d6`, the writer reads
`is_transaction_failed` and `is_transaction_active` using separate lock
acquisitions. If active A becomes Failed between those reads, both predicates are
false and the writer can emit `I`, even though A was never Idle. The existing
`transaction_status()` captures the current status and maps it to a stable
public string under one mutex; reusing it avoids a new public API. The selected
mapping explicitly accepts the three existing strings and reports an internal
error for any unrecognized future string rather than guessing its wire state.
The future Retiring state under #1290 must get its own reviewed mapping.

A test-only callback after the fixed status snapshot, with the session mutex
released, changes active A to Failed before the frame is written. Assert exactly
`Z/T` for the captured active state and verify final session status is Failed.
Separate direct Idle, active and Failed controls verify `Z/I`, `Z/T` and `Z/E`
and public status strings. The callback is a deterministic interleaving, not a
production feature. An instrumented baseline would invoke its callback between
the old status reads and produce `Z/I`; no baseline RED is claimed until that
baseline actually runs. Avoid fixed sleeps and keep output failures observable.

Jev assessed the source interleaving 0.91 and selected the one-snapshot mapping
0.99; it flagged send-time staleness 0.94, which is explicitly allowed above.
Independent review also confirmed the schedule and the need to release the mutex
before the callback. Request/response evidence is retained in the #1290 design
worktree's `target/1290-design/ready-status-{request,response}.json`; Jev is
advisory and does not establish test results.
