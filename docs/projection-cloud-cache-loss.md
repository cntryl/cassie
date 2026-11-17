# Simulated cloud cache loss

This finite #1271 campaign exercises Cassie recovery using the published Midge
0.5.2 `OpenOptions::cloud_simulated` API. It does not simulate power loss or
qualify a real cloud provider. The nine focused controls passed at
`40a08ce25972bc3118d5dcc5dd9fb8f4c668cee9`; exact outcomes and limits are
recorded in the [finite evidence](evidence/replay-read-recovery-2026-11-17/README.md).

## Selected contract

The source-backed Jev decision selects four cases: replay API strict and
replayable writes, and explicit SQL transactions declared strict and replayable.
Each starts with a strictly remote-durable position-1 baseline and kills a child
process after its real position-2 acknowledgement. The parent reaps the child,
hashes the owned simulated remote `cloud_store/`, deletes only the other local
cache entries, and verifies that every remote file and hash is unchanged before
reopening. No lease record is edited.

Strict acknowledgement must recover position 2. Asynchronous acknowledgement
may recover the complete position-1 or position-2 state. Rows, checkpoint and
index contents must agree with that position: event 1 is always present, and
event 2 is absent at checkpoint 1 or present at checkpoint 2. API event-ledger
entries must agree as well; SQL has no event ledger in this fixture. Recovery uses the retained source, starting after the recovered
checkpoint, to reach position 2 without skipping an event; retrying again must
preserve the result. This is a complete-prefix assertion for these two events,
not a bound on the amount or duration of possible loss.

The fixture uses the public 60-second lease TTL with its default 30-second skew
allowance. A 120-second recovery deadline is a hang guard. It retries only the
known writer-lease-held error, without changing the lease or treating another
storage error as retryable. The existing cloud fixtures retain their 300-second
TTL. Acknowledgement is synchronized through a framed child marker; no startup
sleep chooses the crash boundary.

Before the child starts, the baseline must prove exclusive adapter ownership and
successfully call the published mutable `Engine::shutdown` with a 30-second
deadline. Cassie's runtime shutdown alone does not supply that assertion.
The adapter's existing drop path attempts native shutdown for five seconds but
only warns on failure; a failed close can fall through to detached engine
cleanup. The fixture must instead fail on an extra owner, native Busy, timeout,
durability error or other shutdown error. This is an owned fixture handoff,
not an engine-wide WAL-quiescence API or a fix for #1188.

The destructive fixture helper also has deterministic ownership controls. It
refuses foreign, unowned and symlinked cache roots before deletion, preflights
nested entries, and preserves both the full remote tree and an outside-cache
retained-source sentinel. These controls are separate from the four recovery
cases. All three safety controls and the two owned-close negatives passed
in the same nine-control focused run.

## Evidence limits

Each execution must record its exact source, actual outcome and recovered
position. An asynchronous suffix that survives is valid but does not demonstrate
that suffix loss occurred. Process death followed by deleting a simulated local
cache does not establish physical power-loss recovery, real provider semantics,
a global Midge WAL-quiescence guarantee, or production readiness. Those broader
obligations remain under #1271 and #1188.

## Recorded execution

At `40a08ce25972bc3118d5dcc5dd9fb8f4c668cee9` (tree
`9bff39744534ef6a125c6992fee2b5d2aa2ea168`), all nine cloud controls
passed. The actual `--nocapture` observations were API strict **2**, API
replayable **1**, SQL strict **2**, and SQL replayable **1**. Each case then
passed its source-backed row/index/checkpoint reconciliation and retry assertions.
The five existing local process-crash controls also passed. Cloud command time
was 323.84 seconds, including 217.78 seconds compilation; its test harness took
106.06 seconds. These outcomes demonstrate the selected simulated-cache-loss
cases, including observed replayable prefix recovery, without changing the
evidence limits above. Later benchmark-only corrections did not alter these
cloud/process sources; their results retain this exact execution identity.

See the [raw receipts and acceptance map](evidence/replay-read-recovery-2026-11-17/README.md)
for the separate full default gate, corrected eight-profile read campaign,
preserved failed attempts and final-head hosted-check requirement.
