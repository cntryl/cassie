# Simulated cloud cache loss

This finite #1271 campaign exercises Cassie recovery using the published Midge
0.5.2 `OpenOptions::cloud_simulated` API. It does not simulate power loss or
qualify a real cloud provider. Tests are authored but have not run.

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

The destructive fixture helper also has deterministic ownership controls. It
refuses foreign, unowned and symlinked cache roots before deletion, preflights
nested entries, and preserves both the full remote tree and an outside-cache
retained-source sentinel. These controls are separate from the four recovery
cases and have not yet run.

## Evidence limits

Each execution must record its exact source, actual outcome and recovered
position. An asynchronous suffix that survives is valid but does not demonstrate
that suffix loss occurred. Process death followed by deleting a simulated local
cache does not establish physical power-loss recovery, real provider semantics,
a global Midge WAL-quiescence guarantee, or production readiness. Those broader
obligations remain under #1271 and #1188.
