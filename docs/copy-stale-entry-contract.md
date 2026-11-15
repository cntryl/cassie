# Transaction-owned COPY stale entry

A transactional COPY FROM binding retains the session transaction identity,
generation and database captured at bind time. Preserve payload decoding and
CSV header validation before the new check, so their existing malformed-input
diagnostics retain precedence. Immediately before WHERE evaluation and the
write fork, check cancellation and deadline, then validate the owner under the
session transaction lock without retaining it over SQL or storage work. A
matching identity, generation and database in a failed block retains the existing
25P02 failed-transaction error. Reject another session, generation or database,
or an already-idle transaction with the existing stale-owner serialization
error. An unowned autocommit COPY keeps its current behavior. Check cancellation
before and after acquiring the lock; leave the later descriptor, storage,
collection and generation checks in place.

This is a stale-at-entry guard. It does not hold transaction authority during
predicate execution, nor protect an operation from a concurrent COMMIT,
ROLLBACK or ROLLBACK TO after the check. The shared admission, freeze, retry,
retirement and async cleanup needed for that guarantee remain open under #1279;
this guard does not implement replayable SQL durability mode or change its write
policy. Preserve the independently durable sequence behavior of a valid COPY
predicate and existing transaction/error semantics.

Source rationale: `copy/execute.rs::execute_copy_payload` currently evaluates
COPY WHERE through nested SQL before `fork_copy_statement_batch_guarded`
performs the existing owner-generation check. An owner already retired before
execution can therefore reach a sequence function before the stale check. The
regression pairs the exact stale-owner error and an unadvanced sequence with a
valid-owner positive control using the same predicate. Both are authored and
unexecuted until the assembler runs the combined gate.

Jev advisory, preserved under the design worktree's ignored
`target/1279-design/stale-predicate-{request,response}.json`: the conditional
stale-effect concern scored 0.89; a narrow guard was selected 0.66 with explicit
TOCTOU limitation; reusing the complete checked fork as a preflight has a 0.75
risk of new snapshot, locking or resource behavior. Jev does not establish
SQL predicate support or an observed failure. The false-green risk (0.69) is
addressed by the exact-error assertion and identical valid-owner control.

Jev selected the after-decoding/header ordering at 0.95 and preserving the
matching failed block's 25P02 at 0.88 (`target/1282-review/entry-order-{request,
response}.json`). The guard does not detect a same-generation savepoint rollback
and cannot rule out retirement after it returns; both remain #1279 obligations.
Focused tests must cover malformed-header precedence, failed block, already
canceled and expired request, stale generation and session identity, valid
owner, and unowned autocommit.
