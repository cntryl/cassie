# ReadyForQuery transaction status snapshot

Status: #1291; the full ordered local gate passed at `5a1bdbca`; final-head
hosted qualification is pending.
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
false and the writer can emit `I`, even though A was never Idle. The selected
projection exposes the existing status enum only within the crate and captures
its Copy value with one mutex acquisition. The public
`transaction_status()` delegates to that snapshot and keeps its existing strings;
the writer exhaustively maps the typed variants. This makes a future status
variant require an explicit mapping instead of silently reporting Idle. The
future Retiring state under #1290 must get its own reviewed mapping.

A test-only callback after the fixed status snapshot, with the session mutex
released, changes active A to Failed before the frame is written. Assert exactly
`Z/T` for the captured active state and verify final session status is Failed.
Separate direct Idle, active and Failed controls verify `Z/I`, `Z/T` and `Z/E`
and public status strings. These tests passed in the full locked suite at
`5a1bdbca`. The callback is a deterministic interleaving, not a production
feature. An instrumented baseline would invoke its callback between
the old status reads and produce `Z/I`; no baseline RED is claimed until that
baseline actually runs. Avoid fixed sleeps and keep output failures observable.

Jev assessed the source interleaving 0.91 and selected the one-snapshot mapping
0.99; it flagged send-time staleness 0.94, which is explicitly allowed above.
Independent review also confirmed the schedule and the need to release the mutex
before the callback. Request/response evidence is retained in the #1290 design
worktree's `target/1290-design/ready-status-{request,response}.json`; Jev is
advisory and does not establish test results.

Jev selected the crate-private typed projection over string matching (0.91 vs
0.07) and identified a callback-under-lock deadlock risk (0.90). Independent
review also selected exhaustive typed mapping. Receipts:
`target/1291-design/projection-choice-{request,response}.json`. The shared
session enum, snapshot accessor and crate-only re-export are assembler-owned;
the writer, focused tests and this contract are coder-owned.
