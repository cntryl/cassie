# Replayable SQL transactions: design and proof gate

Issue [#1279](https://github.com/cntryl/cassie/issues/1279) adds an explicit
transaction-local projector mode. This document records the intended interface
and the admission design to prove before runtime implementation. The SQL feature
is not implemented or qualified by this document.

## SQL interface

The intended projector form is:

```sql
BEGIN;
SET LOCAL cassie.projection_write_mode = 'replayable';
-- Update projection rows and the checkpoint in this transaction.
COMMIT;
```

The setting belongs to one explicit transaction, not the connection or a session
settings map. Case-insensitive values are `default`, `replayable`, and `strict`.
`default` selects the existing policy; `replayable` opts an entirely replayable
transaction into buffered local WAL or asynchronous cloud upload; `strict`
selects synchronous local WAL or cloud-strict publication. A configured
cloud-strict policy is never downgraded. Mixed transactions require the strictest
actual effect: declared replayable mode cannot weaken DDL, sequence, security or
any other effect requiring strict durability. Unknown effects select strict.
Best-effort/no-WAL is not a projector mode.

Only `SET LOCAL` in an explicit, healthy transaction can set the mode. An
outside or implicit transaction reports `25P01`; an invalid value reports
`22023`; an attempted change after the first write or independently durable
effect reports `25000`. A failed block keeps the existing `25P02` behavior.
`SET LOCAL ... DEFAULT` is the local `default` value, subject to the same
boundary. Ordinary `SET`, startup options, direct session setters and
`set_config(..., false)` cannot set this transaction-only parameter. Generic
`set_config(..., true)` remains unsupported. A targeted session `RESET` or
nonlocal `SET ... DEFAULT` cannot modify the mode; `RESET ALL` retains its
existing behavior for session parameters and does not rewrite transaction mode.

`SHOW` and `current_setting` expose the current declared mode, `default` when
there is no declaration. That declaration is not a durability receipt: mixed
effects may promote the final commit to strict, and `default` retains existing
cloud-background semantics rather than implying cloud-strict acknowledgement.
Output memory/admission must succeed before a setting change, as for existing
session commands. Simple and extended pgwire and embedded execution must agree.

`SAVEPOINT` and `ROLLBACK TO` restore the declaration visible at the savepoint;
first-write history and independently durable effects remain sticky for the
outer transaction. Thus rolling back a sequence advance cannot relax durability
or reopen the declaration window. Full COMMIT or ROLLBACK retires the authority,
including on failed blocks and pool reuse; `AND CHAIN` creates a fresh default
transaction. No effectful statement is repeated merely to settle its mode.

## Durability and recovery

Rows and their projector checkpoint remain one transaction. A buffered success
is not proof that an independently persisted external source offset is safe to
advance: operators need retained source events plus rewind/rebuild from the
recovered checkpoint, or a transaction-specific verified durability receipt.
A SQL checkpoint table does not supply the replay API's event ledger; after an
unknown outcome the projector must use its own idempotency and reconciliation
protocol rather than assuming atomic rows alone prevent duplicate effects.
Unknown native outcomes use existing fencing and recovery rather than claiming a
rollback. Ordinary SQL and autocommit retain their existing policy. PostgreSQL wire COPY
FROM remains supported inside explicit transactions, including its existing
staging, savepoint, sequence and cancellation behavior. A pure transactional COPY joins the containing projector transaction and may
use its replayable policy once COPY owner admission is proven; sequence defaults,
DDL or any other stronger actual effects promote that transaction to strict.
COPY TO may execute a SELECT with independently durable functions, so its
actual effects also require admission and classification. The streamed COPY
owner lives across wire messages through completion, error or disconnect; no
late CopyDone-only permit can protect earlier effects. Autocommit COPY retains
its existing policy and explicit durability barrier.
Postcommit maintenance may still
perform synchronous work before acknowledgement.

## Admission design to prove

A generation-bound transaction owner must be shared by the original session and
all detached statement forks. It carries the declaration, sticky first-effect
state, strongest actual effect, admission state and transaction identity. A
potentially effectful operation obtains authority before forking or doing any
independently durable work; successful publication and failure both release its
permit by ownership/RAII. Actual execution hooks classify side effects,
including sequence/default/function paths and effects preceding a later failure.
Parsing a top-level statement alone does not prove that its nested work is
replayable.

COMMIT closes admission for that generation, waits for already-admitted work
without holding session, global catalog or database-publication locks that the
work needs, and freezes mode and writes as a single consistent authority. The
commit-owned authority selects policy before every native submission.
Ordinary concurrent work cannot change the sealed transaction; a unique
conflict known not to have published the attempted owning-Data transaction may
evaluate its deferred branch under commit ownership and monotonically promote
strictness before a retry. Rollback, rollback-to and disconnect must coordinate with admitted forks; an old owner must not publish into a later
transaction generation. Waiting honors cancellation without leaking permits.
After native submission the existing success/unknown/error rules remain
unchanged.

This is a proof obligation, not permission to hold one nonreentrant mutex across
an entire statement. Nested CTAS and CALL must be able to transfer or release
their own authority before an internally owned COMMIT; they must not wait for
themselves. The implicit CREATE path currently passes an already-held catalog
write guard into COMMIT. Its owner and wait ordering must be made safe before
using the admission protocol there; assuming that every COMMIT begins without a
guard is invalid. Existing same-session cloned reads and updates and independent
sessions must retain their supported concurrency. Query planning, authorization,
conditional Data capture, snapshot hooks and suspended portal lifetimes must not
be moved under a broad execution lock.

## Required evidence before acceptance

Deterministically synchronize SET versus COMMIT, a fork paused before and after
an effect versus COMMIT or ROLLBACK TO, nested CTAS/CALL, implicit DDL's held
catalog guard, cancellation/unwind and independent sessions. Exercise sequence
calls through defaults and functions, savepoints, mixed DDL/security/unknown
effects, pool reuse, and generic SET/RESET/SHOW/current_setting/set_config/startup
paths. Exercise real pgwire COPY in declared and undeclared transactions,
including rollback, savepoint, cancellation and policy selection. Attribute the
actual owning-Data submission policy on local and simulated cloud profiles,
including strict and cloud-strict, no-WAL rejection, ordinary SQL/COPY and unique
retry. A first-native-call observation alone is insufficient when prior schema
epoch or sequence commits can run on that thread. Disk-backed restart tests must
show rows and checkpoint complete-or-absent
and no skipped source event; distinguish process death from power loss and remote
cloud loss. Matched persistent benchmark evidence includes downstream maintenance
and labels foreground versus global barrier counts.

The shared owner, complete effect census, nested transfer and lock ordering have
not yet been established by implementation or executable tests. If they cannot
be proven using the published Midge API and existing boundaries, keep the current
policy and record the exact missing building block; do not silently weaken the
requirements above.

Implicit transaction origin and owner initialization must be atomic. Current
`begin_implicit_transaction` creates a nominally explicit transaction and marks
it implicit under a later lock; a cloned session could otherwise declare the
mode in that gap. An implicit block can also be promoted to explicit without a
new generation, so pre-promotion work and its effects must already be admitted
or safely drained before declaration. A held-catalog implicit DDL commit cannot
wait for an admission that needs the same guard. Cancellation while draining
must retire or fence admitted work before reusing the session generation; the
current immediate rollback-on-COMMIT-error wrapper does not establish that.

### Transferable operation authority: selected direction, not yet proven

Use a distinct execution context for each logical operation. It holds a lease on
one generation-bound transaction authority and can be transferred or shared with
that operation's detached statement fork and nested CTAS/CALL work. Concurrent
session clones must have distinct operation contexts; do not place a mutable
current-command slot on the shared session or rely only on thread-local state.
A transactional COPY owner carries its operation lease across wire messages,
worker transfers and `CopyDone`; an already submitted worker retains its own
lease until the work actually ends. Nested work shares or explicitly transfers
its logical lease so that a commit cannot wait for itself. Acquiring authority
does not itself capture a Midge Data snapshot.

The proposed generation states are open, sealing/draining, frozen and retired.
Beginning a freeze must atomically prevent later admissions. Snapshot and
publication may proceed only after admitted work is resolved and the same owner
and generation are revalidated. Session state may be inspected to identify an
owner, but no session, catalog-publication or database-publication lock may be
held while waiting for that owner to drain. Existing statement execution
continues to use its established catalog/collection/database lock ordering.
For implicit CREATE, reservation and drain before the existing catalog write
guard are a candidate; the held-guard commit must receive already-drained
ownership and not recursively wait. Promotion, SET and nested entry points must
participate in the same authority. Exact lock order and absence of self-wait are
still proof obligations, not established implementation properties.

The implementation of cancellation during drain remains unresolved. A native `nextval` already
submitted may finish and cannot be retroactively canceled by the new owner. The
current COMMIT error wrapper immediately rolls back; it cannot be reused as proof
that an old detached effect has retired. A safe contract must either drain before
the generation can be reused, or retain a closed/retiring authority that rejects
new effects until its prior owners finish. It must state when SQLSTATE `57014`,
`ReadyForQuery`, rollback and a subsequent `BEGIN` become observable and who
owns cleanup. No unowned background teardown or invented native cancellation is
selected.

This is also an async lifecycle obligation. Pgwire error paths call synchronous
rollback from connection handlers, and COPY can still hold its stream binding
when reporting an error. A blocking drain on that reactor can stall unrelated
connections or wait for its own COPY lease. Release or transfer the stream's
own authority at the correct terminal boundary, preserve any already-submitted
worker lease, and use a proven nonblocking/owned drain without waiting under
session or publication locks. The `target/1279-design/owner-transfer-*` Jev
review selected explicit per-execution context; source paths above and
synchronized tests, not that judgment, decide whether this design is safe.

Source boundaries at baseline `bfe7b8f7`: `session/statement_batch.rs:14–61`
constructs a detached transaction-state fork; `session/statement_text.rs`
creates a per-execution clone; `pgwire/connection/copy.rs:225–285,730–843`
retains a binding across asynchronous input; `query_implicit_ddl.rs:19–34`
passes a held catalog write guard into COMMIT; `query_transactions.rs:154–166`
rolls back after a COMMIT error; and `pgwire/connection/transactions.rs:10–19`
can invoke synchronous rollback from a connection handler. These locations
motivate the proposed ownership and cancellation tests and do not establish
implementation coverage.

An independent Jev probe (`target/1279-design/retirement-request.json` and
`retirement-response.json` in the isolated design worktree) favored a fenced
retiring generation over immediate session reset. The later client-behavior section selects this as a target pending proof:
returning `57014` while a generation remains closed would affect `ReadyForQuery`, subsequent `BEGIN`/`ROLLBACK`, pooled
connections, retained-state accounting and cleanup ownership. Source shows COPY
error handlers may still hold the binding while invoking synchronous rollback;
a design must not self-wait or block the async reactor. Compare that option
against an owned asynchronous drain and select exact observable behavior before
implementation. Every option must prevent old forks from publishing after abort,
account any retained state, and preserve actual outcomes of native effects that
were already submitted.

An existing COPY owner is already a revocable pre-fork reservation: the current
`should_reject_a_copy_owner_from_a_retired_transaction_generation` test holds
that owner across rollback and a new BEGIN, then expects the stale fork to fail.
A new protocol must preserve that behavior; making rollback wait on every idle
COPY binding would wait for its own live owner. One candidate distinguishes an
inactive, revocable reservation from an activated effect permit acquired before
fork defaults or other irreversible work. The outcome when COMMIT meets a live
but not yet activated COPY stream must be explicitly selected and tested. This
is not yet an implemented distinction. A last generation check followed by an
unprotected native nextval is insufficient because rollback can occur between
the check and submission.

A count added only to an already-validated COPY reservation does not by itself
establish a new user-visible invariant. The smallest candidate that changes the
unsafe boundary must coordinate activation with COMMIT, rollback and terminal
COPY cleanup; it cannot be treated as complete while those paths still ignore
its authority. Jev's `smallest-slice-*` receipt favored that combined boundary
(.67) and flagged the check/submission race (.93) and waiting on an idle owner
(.83); no resulting runtime behavior has yet been selected or tested.

### Candidate client behavior for concurrent COPY and cancellation

The selected direction is to revoke an inactive COPY reservation when a cloned
session completes or rolls back its generation; its later fork fails as stale,
as in the existing owner regression. An operation that has activated authority
before an effect cannot be treated as merely idle. COMMIT seals new admissions
and waits for already-active work without a session, catalog or publication
lock, then commits a consistent snapshot. The exact protocol response for an
idle wire stream invalidated by a concurrent clone must be tested.

If COMMIT is canceled while an admitted effect may still be running, the
proposed client-visible state is failed/retiring: report `57014`, keep
`ReadyForQuery` at `E`, reject a new BEGIN or effect with the existing failed
block behavior, and permit ROLLBACK to await an owned, cancellable drain before
returning the session to idle. An already submitted native operation may finish;
it is not described as undone. Work not yet submitted must not start a new
effect or publish a fork after abort. If a drain is canceled, the generation
remains failed/retiring. Cleanup of sequence restoration and retained state must
occur exactly once after the last active owner exits. A disconnect must retain
explicit ownership of any still-running native work and its terminal cleanup;
no unowned background work is permitted.

This differs from the current immediate idle reset after a COMMIT error. It is a
selected target requiring source, protocol and executable compatibility evidence,
not a claim that existing clients already observe it. In particular, prove a
nonblocking COPY error path that does not wait on its own stream reservation,
a bounded or admitted owner lifetime, consistent `ReadyForQuery` and pool reuse,
and cancellation before and after native submission. If these cannot be proven,
keep existing SQL durability behavior and leave #1279 open.

Jev selected inactive-reservation revocation (.98), sealing/draining activated
work (.93) and the failed/retiring cancellation candidate (.99); it flagged
client-visible status (.89), idle-stream invalidation (.77), and disconnect
ownership (.91). Receipts are `target/1279-design/client-contract-request.json`
and `client-contract-response.json` in the isolated worktree. Model scores do
not establish protocol compatibility or substitute for these tests.

Streaming portal reads require a separate lifetime from effect admission. Current
pgwire streaming selects a restricted SELECT shape, while effectful SELECT
expressions such as `nextval` use ordinary materialized execution before portal
paging. A suspended result must not retain an effect permit merely for result
delivery, and acquiring the operation authority must not force a Data snapshot
for a proven no-Data plan. Views, nested queries, prepared statements and cursor
execution still need classification and lifetime probes; this observation does
not prove that every SELECT is free of effects.

### Deferred effects during COMMIT retry

A commit-time unique conflict can evaluate `ON CONFLICT DO UPDATE` assignments
only after an initial publication attempt fails. The unique-reservation check
can return a violation before native `tx.commit` is invoked; do not assume the
first native attempt occurred. Assignments may evaluate `DEFAULT`,
`nextval` or other expressions with independently durable effects. The source
path at baseline `bfe7b8f7` is `statement_mutation.rs:124–152` through
`dml_insert_conflict_intents.rs:18–65` to
`dml_insert_conflict_update.rs:288–345`. Freezing the first write option and
blindly reusing it would miss a stronger effect executed by that branch.

The selected target allows only the COMMIT owner, after a unique failure
*known not to have published the attempted owning-Data transaction*, to classify
such work and monotonically promote the transaction's required strictness. It selects the current policy immediately before each
subsequent native submission; it must never downgrade strictness or reopen
ordinary statement admission. An outcome-unknown, fenced or otherwise
indeterminate result is not a known abort and must not be blindly retried. A
more conservative alternative is to classify every possible deferred expression
strict before the first submission; it preserves safety but may downgrade the
performance benefit when that branch never executes. Neither design has been
implemented or tested.

Jev selected monotonic promotion (.98) over conservative preclassification (.02),
flagging deferred sequence effects (.95), typed-abort handling (.91) and commit
authority isolation (.84). Receipts: `target/1279-design/retry-policy-request.json`
and `retry-policy-response.json` in the isolated design worktree. Require a
synchronized unique-conflict test that proves the attempted Data transaction did
not commit, the deferred sequence/default branch executed, and the actual
successful Data submission used strict policy. Include a no-effect buffered
control, typed constraint and unique-index cases, mixed DDL/schema-epoch
witness, and outcome-unknown/fenced non-retry probes. An observer must distinguish
the row transaction from sequence and schema commits; global counters or the
first native policy are not sufficient attribution.

The no-publication statement is limited to the failed owning-Data transaction.
Mixed transactional DDL may already have committed a schema-epoch bump before
the Data write (`query_transactions/repeatable_write.rs:78–85` and
`query_transactions.rs:328–336`); independent sequence effects can also be
durable. Neither fact permits a blind retry of an unknown Data outcome. Source
review and Jev receipts `target/1279-design/unique-provenance-*` require the
phase and identity of each attempt to be observed; they do not establish that no
native writes occurred anywhere in the transaction.

## Implementation status (#1279)

Implemented, pending the ordered gate:

- **Interface.** `SET LOCAL cassie.projection_write_mode = 'default' | 'replayable'
  | 'strict'` and `SET LOCAL ... = DEFAULT` inside an explicit, healthy
  transaction. Errors: `25P01` outside an explicit block (including an implicit
  transaction), `25P02` in a failed block, `22023` for another value and `25000`
  after the first write or independently durable effect. Ordinary `SET`,
  `SET ... DEFAULT`, targeted `RESET`, startup settings, the session setter and
  `set_config(..., false)` are rejected; `RESET ALL` leaves the mode untouched.
  `SHOW` and `current_setting` report the declaration, `default` outside a
  transaction.
- **Ownership.** The declaration and its sticky history live in a
  generation-bound authority on the transaction state, shared by reference with
  every detached statement fork. BEGIN, COMMIT, ROLLBACK and `AND CHAIN` install
  a fresh authority. A savepoint records the declaration and `ROLLBACK TO`
  restores it; first-effect and strict-effect history are never cleared by a
  savepoint rollback.
- **Effect classification.** Classified at each statement's execution
  boundary. `INSERT`, `UPDATE` and `DELETE` are replayable row writes. `SELECT`,
  `SHOW`, `SET` and transaction control are not effects by themselves.
  `nextval`, `setval` and identity or default sequence advances are classified
  where the value is recorded, so they are caught inside any statement. Every
  other statement requires the existing policy: DDL, security, sequences,
  `TRUNCATE`, `MERGE`, `COPY`, `CALL`, SQL-level `PREPARE`/`EXECUTE` and cursors,
  projection and retention commands, extensions and `EXPLAIN`. A staged
  transactional DDL overlay at COMMIT also requires it.
- **Policy selection.** Selected for the transaction's own Data submission
  under the session transaction lock, and re-selected before a unique-conflict
  retry, so strictness can only increase. A declared, entirely replayable
  transaction uses buffered local WAL or the configured lazy cloud policy;
  `strict` uses local sync or cloud-strict; a declared transaction with any
  other effect keeps the existing policy. Undeclared SQL, implicit transactions
  and autocommit are unchanged. Configured cloud-strict is never downgraded.

Not implemented, and therefore not claimed:

- **Admission owner and drain.** There is no generation-bound admission permit
  and COMMIT does not drain in-flight detached forks before freezing. A fork
  that publishes its batch after COMMIT has captured the writes is rejected by
  the existing generation and epoch checks rather than drained. Effects such a
  fork performs independently (a sequence advance) are classified on the shared
  authority but could land after the policy was selected.
- **Replayable transactional COPY.** COPY rows staged in a declared transaction
  keep the existing policy because COPY owner lifetime and admission are not
  proven. This is conservative, not a qualification of COPY.
- **Extended-protocol and pool evidence.** The mode is transaction-owned, so it
  cannot survive COMMIT or ROLLBACK, but no pgwire pool-reuse or
  extended-protocol test was written.
- **Recovery and performance evidence.** No persistent restart test, cloud
  simulation or benchmark was run for the SQL mode; #1271 owns those.
- **Retirement.** Cancellation while COMMIT waits, owner-checked retirement and
  the failed-or-retiring ReadyForQuery status remain under #1290.
