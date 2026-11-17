# Pgwire worker failure authority (#1297)

## Current integrated contract

The worker-only increment below is historical. The integrated #1290 repair now
carries operation ownership through outer simple, extended and COPY handlers,
terminal retirement, quarantine and portal publication/cleanup. Retained worker
leases and metadata reservations follow the actual last owner. See the
[transaction authority contract](transaction-authority-midge-contract.md) and
[qualification record](evidence/runtime-closeout-2026-11-17/README.md) for exact source identities, actual gates and remaining
published-Midge limits.

## Historical worker-only increment

This child of #1295 / PR #1296 covers only failure completion inside
`execute_pgwire_query`. The parent merged as
`42a819108c8fcd6d7e4447ec3ef4e088fb283abe`. The full locked suite passed at
`11da367a`; affected worker tests, strict Clippy, formatting, touched-test policy
and inventory passed at `01c39ea5` after test-only semicolon corrections.
Hosted qualification was pending in that increment's original record; current
publication-head outcomes are tracked in the integrated qualification record.

A healthy request retains the existing nonblocking running-query admission,
including its recovery recheck when the slot is unavailable. The operation
captures its transaction authority only after admission and the existing
statement-timeout snapshot. Capture is the ownership linearization point: an
independent replacement before capture becomes the captured owner. On failure,
the worker marks Failed only if the root transaction, owner and rollback epoch
still match. A matching current-owner error or cancellation still fails the
block. Metrics and result/error precedence stay in their existing order.

The captured authority travels through the parsed and text pgwire entry points,
including bound statements, nested execution and implicit standalone DDL. An
operation adopts its own new transaction and successful savepoint mutation only
at the existing operation-aware state transition; promotion of an independently
started implicit transaction and stale epoch changes do not grant authority.
The nested CALL witness covers the supported idle-session path and verifies a
present operation authority with no transaction owner. Existing transaction
semantics reject CALL inside an active block, so that witness does not establish
nested CALL propagation for an active owner.

When the existing pre-admission recovery checks select the quarantine path,
its legacy execution and completion remain unchanged. Recovery can also latch
later: existing terminal recovery handling can roll back a transaction without
an owner claim. The outer simple, extended and COPY protocol error handlers,
portal cleanup, commit/rollback retirement, and effects already in progress are
not protected by this worker-only increment. #1290 remains open and no complete
wire or replayable SQL safety is claimed.

Synchronized probes must cover an old worker error after replacement, matching
owner errors and cancellation, own start/promotion and savepoint transitions,
saturation, timeout and recovery recheck ordering. A test seam must observe
the actual capture or completion boundary and release any session lock before
waiting. The bounded probes passed locally; no instrumented baseline RED was
observed.
