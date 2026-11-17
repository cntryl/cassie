# Query failure and transaction authority

Current integrated source and actual qualification are recorded in the
[qualification record](evidence/runtime-closeout-2026-11-17/README.md) and the [transaction authority contract](transaction-authority-midge-contract.md).

## Historical converted-completion prerequisite

The bounded #1295 increment below passed its recorded local validation. That
increment did not complete #1290; its old blockers describe that earlier scope. The full
locked suite and strict Clippy passed at `a59e7c65`; formatting, touched-test
policy and inventory passed at `94e171a4` after test-only ordering and comment
changes. No instrumented baseline RED was observed. Its preceding
COPY prerequisite #1288 / PR #1289 merged at
`12431805adee449eb2775116faadde724c7ae8d6`; atomic ReadyForQuery and writable
decision prerequisites #1291 and #1292 merged through PRs #1293 and #1294,
with current parent `630d1a41f8557fee3e5448786fcb3c001b7862f0`. This is one prerequisite for
#1279; it does not implement effect admission, retirement or replayable SQL
durability.

An operation's execution and response handling have authority to fail or
implicitly roll back only the transaction that operation owns. Capture session
identity, transaction identity and status together under the transaction mutex.
When an error is completed, compare that authority and transition the matching
still-active transaction under that same mutex. If an old query from A finishes
after A is gone and a clone has begun B, deliver the original query error and
record its metrics without marking or rolling back B. Errors and cancellation
belonging to the still-current transaction retain the existing Failed/25P02
behavior; a matching implicit transaction retains its existing rollback behavior.
The comparison and transition must be atomic, not a check followed by a separate
unconditional mark or rollback.

An operation starting while Idle owns no transaction merely because another
clone starts one later. Conversely, a transaction actually begun by the same
operation, including explicit BEGIN, an implicit transaction, or a new block
created by AND CHAIN, must be attached to that operation at the transition under
the transaction mutex. Implicit-to-explicit promotion retains the existing
transaction authority. Nested CTAS, modifying CTEs and procedure calls must
explicitly propagate or transfer their logical operation authority; no global
session-wide mutable current-command owner may be shared by concurrent clones.
Prove which nested transitions belong to the outer operation before depending on
this rule. Capture and context adoption must not silently adopt a transaction
created by an independent operation.

The existing state has a `unique_owner_id` refreshed on true transaction start
and end and retained on implicit promotion, and a generation which currently
wraps. A selected implementation may reuse the former with the identity of the
owning session mutex; detached statement forks copy its value into a different
mutex. Do not claim absolute no-ABA protection solely from a wrapping generation
or assume a copied ID is a session identity. Select and test the exact token
lifetime and acquisition mechanism before runtime changes.

Authority must survive the actual execution, conversion, and response boundary:
`query_lifecycle.rs` converted and COPY-WHERE paths, `query_prepared.rs`, simple
and extended pgwire worker return, result/frame encoding, outer error handling,
COPY error cleanup, and any error-driven portal cleanup. A protocol read failure
without a dispatched worker uses its own contemporaneous message authority.
Do not clear or roll back a newer generation's portals/session on an older error.
Retain normal portal cleanup for a matching owner and preserve actual diagnostics,
output-admission ordering, cancellation, and recovery-required quarantine.
Audit other internal `mark_transaction_failed` or rollback paths; an outer guard
alone is insufficient if an inner stale operation can mutate the new transaction.

Test first with no sleeps. A converted query successfully evaluates in A, then a
barrier holds its converter while a cloned session rolls A back and begins B;
when the converter returns an injected error, assert that error is delivered and
B stays healthy and can commit its own row. Add current-owner error and cancellation
controls, Idle raced by an independent BEGIN, a transaction actually begun or
chained by the operation before conversion fails, implicit promotion, and a fresh
session. Use matching simple/extended pgwire and COPY failure/response probes,
including portals and matching implicit rollback, to verify ownership survives
worker and async boundaries. Synchronize protocol cases at real failure/response
boundaries; test current-thread reactor cases under an external watchdog where
needed. No new test is an observed RED until executed.

Jev selected explicit per-operation authority (1.0) with risks for the existing
stale-completion schedule (0.94), own-BEGIN attribution (0.95), idle fallback
(0.96), wire handlers (0.93), shared command state (0.91) and non-atomic nested
transition attribution (0.90). Request and response are preserved under the
isolated worktree's `target/1290-design/failure-authority-{request,response}.json`.
These are advisory; source review and executed regressions decide correctness.

## Conditional implicit rollback is a separate proof obligation

An identity check followed by today's `rollback_transaction()` is not atomic.
That method first calls `restore_sequence_restarts()`, which takes and invokes
shared session restart callbacks before `rollback_transaction` acquires the
transaction mutex and resets its current state. A transaction can change in
between the check, callback restoration and reset; the old error handler could
consume restart state or reset a replacement. A safe implementation must claim
retirement for the matching authority before any of those effects, prevent
replacement or misattribution until that claim is resolved, and make sequence
restart and portal cleanup authority-safe. Prove lock order, cancellation and
client-observable implicit rollback without holding the current-thread reactor
across native work. The existing UUID alone does not provide this lifecycle.
This is source-supported risk, not an executed reproduction.

## Selected claimed-retirement reuse behavior

When an operation conditionally rolls back a matching implicit transaction, it
must first claim that exact authority under the transaction mutex. While claimed,
transaction reuse is denied rather than reporting the session Idle before the
old cleanup completes. A concurrent BEGIN must return `25000` (`InvalidState`)
with `transaction is retiring; retry after rollback completes`, without changing
the claimed authority, its generation, or any transaction state. The caller may
try again after retirement actually completes. A competing rollback may wait or
return without taking ownership, but must not clear another operation's claim;
its exact reporting and cancellation behavior still require selection. Once all
claimed synchronous cleanup is complete, new BEGIN succeeds normally.

This intentionally changes a concurrent cloned-session interleaving and needs
client and wire qualification. A still-retiring transaction must not be
advertised as Idle; the target ReadyForQuery status is `E` until retirement has
completed, with exact protocol transitions and disconnect handling requiring
real wire tests. Cleanup callbacks and their owned worker must survive requester
cancellation until actually finished. An OutcomeUnknown is not proof that native
work stopped; retain the storage recovery fence and its existing reconciliation.
Do not hold the transaction mutex or a current-thread Tokio reactor while waiting
for cleanup. Thread/worker ownership, memory bounds, shutdown, nested CTAS and
lock order remain proof obligations; no detached unowned cleanup task is allowed.

Alternative waiting at BEGIN may preserve a caller's apparent sequential flow,
but current embedded entry points are synchronous and existing pgwire error
handlers can call rollback from a current-thread reactor. A safe wait requires an
owned, bounded, cancellation-aware async protocol across all entry points and
must avoid waiting for its own COPY lease. Prematurely allowing B is unsafe when
A's restorer can still mutate sequence state. Jev selected the nonblocking busy
contract 1.0, flagged observable compatibility 0.92 and early-reuse risk 0.93;
`target/1290-design/retirement-reuse-{request,response}.json` preserves the
request and response. This is a selected target, not executable proof.

Regression design: park a test restorer only after A's retirement is actually
claimed; a second rollback signals only after it observes that same claim. An
independent coordinator verifies concurrent BEGIN gets exact `25000` and the
same generation remains claimed, then releases the first callback, joins both
rollback attempts and proves a new B can begin, write and commit. The test must
release all owned workers on every failure and must not join a contender before
releasing the first callback. A separate test captures A's error authority, ends
A and starts B before A's conditional retirement tries to claim; stale cleanup
is a no-op and B remains healthy. A test callback proves ordering, not actual
native sequence restoration or WAL safety; those need separate probes.

The busy rule applies to SQL BEGIN as well as the embedded transaction entry;
its execution/error wrappers must not mark, release or replace the Retiring
claim. BEGIN's selected `25000` takes precedence over a generic failed-block
rejection in this state. A direct method probe alone is insufficient: exercise
SQL BEGIN while cleanup is paused, then verify the claim/identity and subsequent
healthy transaction. Separate simple and extended pgwire probes must observe
`25000` plus ReadyForQuery `E`, matching-owner implicit cleanup, and no mutation
of a replacement; the supported real wire interleaving and external watchdog
remain to be demonstrated rather than assumed from an embedded clone.

A focused handler-level probe can exercise actual simple-query response frames
without claiming a full socket test: create a `SessionState` with a registered
backend, pass the same cloned session to `simple_execution::run_batch`, then
call `transactions::finish_query` and decode the emitted error and ReadyForQuery
frames. A coordinator holds a claimed implicit rollback using the test-only
restorer while the handler receives BEGIN; require `25000`, Ready `E`, the same
claim, and a usable new transaction only after cleanup completes. The test uses
the external watchdog and drops its current-thread runtime before its completion
receipt. This bypasses authentication, socket transport and full connection
lifecycle. Extended protocol, COPY and network-level interleavings remain
separate acceptance obligations.

Retiring must extend beyond transaction-field detachment to completion of the
old session's schema-context retirement, unique-owner release, settings/search
path restoration and transaction-object cleanup. The current implementation
sets Idle before these steps; allowing B then permits A's later shared-session
cleanup to alter B. A separate externally supervised test seam pauses before
that cleanup and requires the same `25000` and unchanged claim until it finishes.
This test is a lifecycle witness, not an actual native-storage failure probe.

An additional handler probe feeds actual extended Execute and Sync frontend
frames through an owned loopback reader into `handle_ready` with a prebuilt
BEGIN portal and the same injected session. It checks entry into and exit from
the production Sync-wait route after the Execute error. It expects one 25000
error, one final Ready `E`, no CommandComplete and the same retirement claim.
This exercises extended dispatch and response framing, including Sync cleanup;
it still bypasses connection startup/authentication and does not by itself prove
stale-query protection for a replacement transaction or COPY error handling.

A separate simple-handler stale-output probe uses a test `AsyncWrite` that
signals only when A's successful SELECT reaches its RowDescription write,
then yields. A coordinator rolls back A and begins B before the writer returns
an injected resource-limit error. The actual simple-query error wrapper must
emit the original `54000` and Ready `T`, leaving B healthy and able to commit
its row. This is a handler-level wire-frame test with an injected write error,
not a full socket, native storage, or transport-disconnect witness.

## Commit publication and competing rollback

For a row-only READ COMMITTED COMMIT without transactional DDL, the commit path
has no catalog publication guard. Its collection-gated `publish_transaction_rows`
can succeed and release those gates before the unconditional
`session.commit_transaction()` resets shared transaction state. A source-backed,
unexecuted schedule pauses A immediately after that confirmed native publication;
a clone rolls A back and begins B; A resumes and its old finalizer can reset B.
A row already published by Midge is not undone by clearing session state. A
conditional finalizer alone would protect B from that reset but would still let
A's native publication overlap a reported-successful rollback or capture mixed
transaction generations. Do not present that narrow check as transaction
atomicity or as a known-not-committed result.

The candidate full lifecycle must atomically claim A as Committing before
reading commit snapshots or submitting native work, retain that authority through
known outcome and all sequence/schema/session/portal cleanup, and coordinate
competing transaction transitions without holding session, catalog, collection,
or reactor locks while waiting. A conflicting rollback must not report success
while A's native outcome remains unresolved. Jev favored a nonblocking busy
error over an owned cancellation-aware drain (0.74 versus 0.24); a late-only
finalizer was 0.01, and immediate successful rollback 0.0. This is an advisory
candidate, not an adopted runtime contract: exact error/status, same-session
clone compatibility, nested/internal ownership, implicit DDL's pre-held catalog
guard, commit cancellation and actual native OutcomeUnknown quarantine remain
unproved. Existing `rollback_transaction()` returns `()`, so a busy result
requires a source-audited API and caller transition; do not silently translate a
conflicting rollback into successful `ROLLBACK`.

First author an instrumentable commit-boundary probe after the native Data
publication succeeds and collection gates have been released. A baseline-only
instrumented reproduction can let old rollback and new BEGIN overtake that
pause to expose the existing finalizer race; do not impose that interleaving on
a corrected claim, which must prevent it. The target probe should instead wait
for a signal emitted after the real commit claim and confirmed native success,
then verify a competing rollback is rejected or safely waits according to the
selected contract, no B is admitted early, and the coordinator releases A before
joining any waiting peer. Verify A's stored row before release and a later B's
state/row after finalization. Separate probes must cover pre-submit failure,
cancellation, native OutcomeUnknown and recovery, CTAS/CALL, implicit DDL and
held catalog locks, multiple competitors and task ownership. No claim is made
that a precommit hook proves confirmed native publication or that the described
baseline has executed. Receipts:
`target/1290-design/commit-retirement-{request,response}.json`.

The authored, unregistered baseline-only probe is kept separately in
`src/app/query_failure_commit_baseline_probe.rs`; it is not part of the target
suite. To execute it against the old source, the assembler must temporarily
register that module and add an equivalent test-only hook after successful Data
publication and collection-gate release. The probe deliberately lets an old
rollback and new B overtake A's paused finalizer; this is a baseline witness,
not the target contract. Neither the hook nor the probe has been run.

The target test instead depends on a future scoped test-only commit observer.
The observer is installed in the specific worker, keyed to the Cassie/Midge
instance and captured transaction identity, and signals only after the actual
native Data publication succeeds and collection gates are released. A separate
scoped rollback-conflict observer reports either `Rejected` only after the
actual conflict decision, or `Waiting` only after the waiter is registered
against A's real commit claim. It must not signal merely on approach to a lock.
Both observers are RAII-cleaned and absent from production behavior. The test
requires the actual A claim and visible row, observes a competing rollback
without requiring it to complete while A is paused, releases A before joining
any waiting peer, and proves subsequent B can write and commit. Rejection's
exact SQLSTATE and waiter outcome remain open until the competing-commit
contract is selected; no test may infer native success from a precommit pause.
An external watchdog bounds the whole child process if a faulty implementation
blocks in a contender or join. These are authored interfaces for future
implementation, not claims that such hooks or claims currently exist.

The conflict observer must also emit `Completed {
pre_transition_committing_owner, succeeded }`. At the actual authoritative
rollback decision or mutation boundary, capture the pre-transition owner and
status under the session mutex **before any rollback mutation**; the field is
`Some(owner)` only if that pre-state is actually Committing. Preserve this
immutable observation with that exact operation through any later unlocked
cleanup and report it with the actual terminal returned result; do not assume
cleanup cannot fail or resample after the mutation. A waiting rollback captures
its actual post-wait pre-transition state, not merely its initial collision.
Report without blocking the transaction mutex. A `Waiting` notification alone
is insufficient; a buggy rollback could still report success while A's claim
remains active. The test requires the recorded result to match the actual
returned result, rejects success while the same A was Committing at the
rollback transition, and separately requires a `Rejected` decision to return
an error. This requires real implementation instrumentation, not a
test-synthesized status sampled after a worker was scheduled. The bounded event
channel must admit both decision and completion without waiting while the
session mutex is held.

A focused negative-control helper must exercise that very same observer against
a real locked test claim while deliberately clearing it before returning success.
The resulting observation must still carry the original A claim and the test
must classify that completion as unsafe. It is insufficient for a fake helper
to construct a `Completed` value or sample the claim only after the clear. This
seam is not yet implemented or executed.

## Frontend message admission authority

For an authenticated pgwire connection, select a fresh operation authority after
`handle_ready` has a first frontend byte available and has selected the session,
but before awaiting the remainder of that frontend frame. Capture the current
session identity and transaction authority together under the session mutex.
This is the transaction at handler admission, not a claim about the exact
physical instant that the first byte arrived. A fragmented frame admitted in A
must not fail or roll back replacement B if the remaining bytes later produce a
protocol error. A frame admitted when B is current must still follow normal B
error semantics. Handshake/startup failures before a session exists retain their
existing handling. A simple-query batch or extended Execute may update its
operation authority only when that same operation actually starts or promotes a
transaction under the mutex. A COPY input stream retains its originating COPY
operation authority across input frames rather than adopting whichever session
happens to be current for each fragment.

A deterministic handler probe should send only an extended message tag, wait
for a test observer to report the actual session and operation authority after
admission, replace A with B on a clone, then send a malformed remaining frame and
Sync. Require the original protocol error, a single Ready `T`, and healthy B
with its staged row and SQL cursor intact. A pre-existing B connection-portal
witness is separate. The observer must be test-only, scoped by
backend/connection identity and one operation, RAII-removed, and signal only
after the actual token is captured and the transaction mutex is released. A
partial-write scheduler delay or merely reading the tag is not a substitute for
that event. Supervise the current-thread reactor in a child watchdog and include
same-owner malformed-frame and fragmented-COPY controls. This test validates
the handler boundary only, not socket authentication or a physical arrival-time
ordering. The two handler probes are authored and unregistered; the observer
and fragmented-COPY control are not yet implemented or run.

Jev selected this admission boundary 1.0 and flagged the risk of decoding first
and then adopting a replacement (0.91), plus a possible new per-operation heap
or resource-admission ordering change (0.62). It did not treat the token capture
as the exact physical first-byte arrival, or fresh authority per COPY fragment,
as safe assumptions. Receipts:
`target/1290-design/message-authority-{request,response}.json`.

The authored handler probe creates B's staged row and SQL cursor before the
old fragmented error completes. It also inserts a sentinel connection portal
after that error solely to check that the following Sync does not clear it; the
latter is not proof that an already-existing B portal survived the old error.
A separate pre-existing-portal/old-error witness remains required. The
backend-keyed admission observer is a future test-only seam, not an existing API.

## Implementation boundary and remaining proof

The smallest candidate for the full issue is a coupled change, rather than an
entry-only failure mark or an implicit-rollback claim in isolation:

1. Carry one operation-specific authority through query execution and wire
   response handling. Adopt a transaction started by that operation under the
   transaction mutex; nested work must transfer that authority explicitly.
   Cover converted and prepared execution, message admission, COPY streams,
   output errors, and owner-scoped portal cleanup. A cancellation handle is not
   an operation identity: callers can reuse it across requests.
2. Claim COMMIT before its snapshots and native publication, and claim matching
   rollback before sequence restoration. Keep the claim through the actual
   native outcome and all shared cleanup; publish reusable Idle only after that
   work is complete. Prove that concurrent commit, rollback, settings changes,
   and transaction creation cannot mutate or retire another operation's claim.
3. Admit effects before sealing and transfer the same logical owner through
   nested CTAS, modifying CTEs, implicit DDL and COPY. Prove each internal
   commit/rollback can complete without waiting on its own owner or acquiring
   locks in the opposite order. An implicit table creation currently holds the
   catalog publication write guard while calling commit, and modifying CTEs can
   begin and commit a transaction before their outer executor returns.
4. Keep started cleanup work owned until it actually finishes, including when an
   async caller is canceled or disconnects. Do not hold the session mutex or
   current-thread reactor across native work. Define bounded worker ownership
   and shutdown before promising cancellation response timing; a dropped
   `spawn_blocking` join handle is not evidence its operation stopped.

This is an implementation map, not a lock proof or authorization to weaken the
existing behavior. `QueryExecutionControls` has per-request tracked memory and
is cloned into nested work; adding a separately allocated context before query or
output admission must preserve budget accounting and error ordering. Also,
`execute_copy_where_with_controls` independently acquires a running-query slot:
a draining waiter must not consume the only slot needed for the already-admitted
COPY to finish. These are Cassie-side lifecycle and resource obligations, not a
demonstrated missing published Midge API.

Before runtime changes, source review must establish the lock graph and actual
claim ownership for the held-catalog and nested paths, plus cancellation and
unknown-outcome handling. Required synchronized probes include the authored
post-native COMMIT/ROLLBACK contention and claimed-retirement tests, a
one-worker COPY/rollback interleaving, nested modifying CTE and implicit DDL,
old operation versus replacement transaction, shutdown with an active cleanup,
and native unknown-outcome fencing. The baseline-only COMMIT probe remains
unregistered; it must be separately instrumented and run before claiming a
reproduction. No current test or observer establishes these obligations.

For this scope Jev chose the coupled candidate (0.65), with 0.35 on no supported
candidate before another invariant is demonstrated; it assigned 0.77 to nested
seal/admission risk, 0.79 to losing cleanup ownership after dropping a blocking
handle, and 0.68 to resource/precedence changes from early context allocation.
The request and response are in
`target/1290-design/implementation-boundary-{request,response}.json`. Source
and executable evidence, rather than those probabilities, determine whether a
particular implementation is safe.

## Converted-completion candidate

A possible first runtime increment is limited to the completion performed by
`finish_converted_query`: it records the original result and metrics, and marks
Failed only when that operation's actual owner is still current under the same
transaction mutex. This includes SQL conversion, explain conversion, and the
nested COPY-WHERE call using its originating operation's authority. It does not
change prepared/wire error wrappers, direct inner failure marks, rollback,
commit, portal handling, or claim to complete #1290. Those paths remain unsafe
until the coupled work above is implemented and qualified.

An entry snapshot alone is insufficient. A successful BEGIN or AND CHAIN by
the same operation must adopt the new owner at the actual transaction-start
transition, under the mutex; implicit promotion retains the same owner.
Conversely, a different operation that starts B while this operation was Idle
must not be adopted. A nested COPY predicate must inherit its actual COPY
operation's authority, never whichever transaction happens to be current when
the nested query starts. The existing controls contain a cloneable cancellation
handle, but that handle is not a unique operation identity.

Any implementation must establish the owner slot's creation, sharing, lock
order, accounting and lifetime before code is selected. In particular,
`execute_sql_converted` makes fresh controls before worker admission, whereas
explain checks recovery and worker admission before making controls. COPY WHERE
receives caller-supplied controls. Avoid changing their existing cancellation,
quarantine, authorization, resource-limit and query-admission error precedence;
do not reuse a mutable authority across independently invoked operations. The
existing statement-text owner clones controls after SQL preflight and resource
admission, so it cannot alone represent failures before that point.

The authored conversion probes cover replacement, Idle versus independent
BEGIN, same-owner failure and cancellation, own BEGIN and AND CHAIN, and implicit
promotion. Additional authored probes exercise replacement during explain
conversion, while a nested COPY predicate is paused and then canceled, and
across two independent operations that reuse one cancellation handle.
Both shared-handle schedules are necessary: one retains B while old A fails;
the other requires B's own error to fail B before C replaces it and A returns.
These bounded probes and their same-owner, nested-control, early-error, own-start
and resource-accounting controls passed in the locked suite at `a59e7c65`. The
COPY predicate probe covers nested execution, not the full COPY wire lifecycle. No baseline RED or complete #1290 fix is claimed.

The authored COPY-origin discriminator binds COPY in A and requires a future
test-only accessor to read A from the real bound operation authority. A scoped
observer must pause after the real owner check and before predicate evaluation,
without retaining the transaction mutex. After A is replaced by B, the same
bound COPY resumes and its nested predicate reports the original division-by-zero
error; B must remain healthy and retain its identity. The changed-owner probe
also installs distinct outer controls and verifies that the existing ambient
publication scope retains those controls immediately before the payload call.
Source currently forwards those controls to the nested predicate, but the
pre-call check alone cannot prove a future implementation preserves them. A
future observer at actual nested entry and a distinct outer-versus-binding
cancellation or deadline control must verify that precedence. COPY's originating
authority must survive it. A same-owner control uses
the identical predicate and must fail A. The changed-owner test runs under the
existing child-process watchdog so a broken pause or worker cannot hang the
parent indefinitely. The accessor, observer and test registration are present
in this candidate, and both probes passed in the locked suite at `a59e7c65`. This probes operation authority at nested entry;
it does not establish #1279's stronger protection of in-flight COPY effects.

### Selected explicit context candidate

The selected candidate is a separately owned, per-operation context with weak
identity for the originating transaction mutex, rather than a slot whose
lifetime is tied to `QueryMemoryTracker` or the ambient publication scope.
The candidate creates and transfers this context; the bounded local checks passed. Its
initialization must capture the current owner and status together under the
root transaction mutex;
an Idle operation initially owns none. An actual own BEGIN/CHAIN must adopt its
new owner in that same start critical section; a failed BEGIN must not adopt an
independent clone's transaction. Completion compares root identity and owner
under the transaction mutex, then marks only the matching active transaction.
Implicit promotion and the lifetime after an internally owned transaction ends
still require proof. A detached statement fork copies a UUID but uses a different
transaction mutex, so UUID alone is insufficient.

One explicit signature path to evaluate is
`query_lifecycle::{execute_sql_converted, explain_sql_with_cancellation_converted}`
through `query::{execute_sql_core, explain_sql_core}` and
`query_bound::{execute_parsed_statement_core,
execute_parsed_statement_core_with_parameter_oids, execute_bound_statement,
execute_bound_statement_inner, execute_bound_query, execute_bound_transaction}`
to `query_transactions::execute_transaction_statement` and
`session::begin_transaction_with_origin`. Execution clones created by
`with_statement_text` and nested executor calls must receive the same logical
context; in particular procedure CALL passes through
`dml_command::execute_call_procedure_command` and `execute_sql_with_controls`,
while modifying CTE and CTAS can start or finish internal transactions. Any
session-clone carrier must also cover errors before `with_statement_text` and
distinguish the root session from detached forks.

COPY needs an explicitly retained originating context through
`bind_copy_from_stdin` and `CopyFromBinding`, then
`execute_copy_payload` → `filter_rows` → `execute_copy_where_with_controls`.
The binding crosses async input and is moved into a `Send + 'static`
`spawn_blocking` closure; a stack borrow alone cannot survive that boundary.
Synchronous functions may borrow the owned context while it remains alive.
The nested call must not recapture the then-current transaction or derive the
authority from whichever query controls happen to be ambient.

This is a source map, not a complete signature list or a proven layout. The
new owned context's allocation, accounting, retention through output, and
early-error ordering must be resolved. Verify all owner-slot access takes locks
in a documented order, never holds them across storage, waiting or callbacks,
and cannot reverse root/fork order. Quarantined transaction cleanup and the
prepared/pgwire failure handlers remain outside this bounded increment. Jev
favored an owned transferable context over borrowed-only (0.98 versus 0.01),
and flagged 0.83 risk if an abandoned async waiter loses ownership of a started
blocking operation. Receipts:
`target/1290-review/context-options-{request,response}.json`. No executable
proof or production behavior is implied.

### Conditional by-value implementation candidate

For the bounded converted/COPY-predicate increment, select an owned-by-value
context subject to source review and compilation of the complete call graph. A
synchronous converted or explain operation owns it on its stack and passes
explicit shared borrows into execution; COPY binding owns it through input and
moves it into the existing blocking worker. The context contains weak root
transaction-mutex identity and a fixed-size synchronized owner slot. This avoids
requiring a separate `Arc` for the context on these paths; it does not establish
that the surrounding query performs no allocations, nor waive accounting for
any newly retained data. Scoped executor fanout must join before the owner drops.
COPY currently reserves `size_of::<CopyCommandOwner>()` plus Arc overhead during
its guarded owner capture. Placing the context inside that owner changes the
charged size; placing it in `CopyFromBinding` changes a separate retained
allocation. Either layout needs a source-backed accounting choice and
just-below/above budget witnesses. Capture the UUID while the existing guarded
transaction snapshot is held; do not reenter that same mutex.

The selected binding-layout candidate retains the by-value authority and its
reservation in every COPY FROM binding, including autocommit. Let B be the
actual `size_of::<CopyFromBinding>()`, including all fields and layout padding,
and S be `size_of::<CopyCommandOwner>()`. After existing target binding and
force-column validation succeed, reserve B−S when an owner is present and B
when it is absent, using checked subtraction and the binding's own query
controls. The present owner separately reserves S plus Arc overhead on those
same controls. This charges the full inline Box payload without charging S twice and
also includes the reservation field itself; it newly charges other binding
fields that previously had no separate binding charge. Verify the project's
Box/allocator accounting convention and identical tracker/lifetime before
implementation. This is not a claim that allocator metadata or the separately
allocated `Arc<Cassie>` is accounted by this charge. After existing validation,
check the binding controls for cancellation/deadline before reservation; a
newly observed interruption takes precedence over the new `54000` boundary.
Allocate only after admission. Move the reservation with the binding into the
worker and release it only when the binding is dropped. This explicitly adds a
retained-memory charge: tight-budget requests may newly fail with `54000` at
binding or later as a result of the smaller remaining budget. Earlier parse,
authorization, recovery, target and force-column errors retain their ordering;
ambient controls must not be charged in place of the binding's controls. Prove
the actual added layout and reservation lifetime before qualification, and
measure the usage at the actual admission boundary: for added charge C and
remaining budget R, R=C must admit and R=C−1 must fail. Earlier provisional and
final owner snapshots can have a peak P above the current usage U, so a budget
of U+C−1 alone may fail before reaching this boundary. A focused test may hold
an additional reservation on the actual binding tracker only at this boundary,
provided the hook proves it reached the real admission after validation and
allows prior P; it must test the real binding result and not allocate a large
input merely to create pressure. In addition to a
focused reservation control, exercise real binding at both limits, an earlier
validation error, autocommit, nested ambient controls and binding/worker drop.
Do not infer post-return retained usage from the admission-point value, because
binding temporaries may be released. This is a selected resource contract, not
a measured size or qualified behavior. Jev selected the nonoverlapping layout
(0.94), while flagging allocator convention (0.89), tracker/lifetime (0.79),
and pre-reservation measurement (0.86). Earlier Jev flagged tight-budget (0.95),
wrong ambient tracker (0.91), helper-only evidence (0.97) and reservation-field
undercount (0.91) risks. Receipts:
`target/1290-design/copy-{accounting,layout}-{request,response}.json`.

For adoption or completion, acquire the root transaction mutex before the
owner slot, and release both before waiting, storage, callbacks or async work.
Initialization captures root identity and the active owner together. Only the
operation actually performing a successful start may replace its slot with the
new UUID under that same transaction lock; a failed begin does not adopt, and
promotion retains the UUID only when it is already that operation's owner. If
A captured Idle and an independent clone starts implicit B before A's BEGIN,
the existing BEGIN can promote B, but A must not adopt B for failure completion.
An authored post-capture/pre-BEGIN synchronized test exercises that distinction;
the keyed test hook is implemented and the test passed at `a59e7c65`. Preventing the
promotion itself is outside this bounded completion fix. A detached fork must
not overwrite the root slot using a copied UUID or acquire fork and root locks
in an unproved order.
Completion compares the weak root identity and owner under the root mutex and
marks only a matching active owner. Own-start, replacement, cancellation,
quarantine and early-resource/error precedence remain required witnesses.

Beyond the application paths above, passing a borrowed context to nested
execution affects `query::execute_physical_statement_with_parameter_oids`,
`executor::run_with_session_parameter_oids`,
`execution::entrypoints::run_with_session_controls`,
`modifying_ctes::{StatementInputs, run}`, and
`dml_command::{execute_command, command_outcome, execute_call_procedure_command}`
and their intervening dispatchers. Existing independent callers must explicitly
create a fresh root or pass no operation owner according to their contract;
CALL and COPY must transfer the originating owner rather than infer it from
reused controls. The exact executor signatures and direct callers must be
audited before qualification; a lifetime-free `CassieSession` cannot simply
store a borrowed reference.

The current COPY worker returns only its result to `finish_simple_copy`; the
outer error handler still acts on the session. Moving the sole context into the
worker does not make that handler owner-safe after an error, join failure, panic,
or dropped awaiter. A full wire solution needs a separately proven ownership
envelope or other lifecycle design, including cleanup and shutdown. This slice
does not claim that solution or #1290 completion. Source review found no
inspected converted/COPY-nested path that requires a separate `Arc`; Jev favored
by-value feasibility (1.0) and flagged outer-context loss (0.70). Receipts:
`target/1290-review/by-value-review-{request,response}.json`.

### Savepoint recovery authority

An operation's owner includes both transaction UUID and rollback epoch. A
successful `ROLLBACK TO` keeps the UUID but advances the epoch and restores an
active block. An older operation's error must not fail that recovered block.
An operation performing its own rollback-to may adopt the resulting epoch only
if its pre-transition root identity, UUID and epoch match the actual root state
under the transaction mutex. A previously stale operation must not acquire the
current epoch merely by attempting another rollback. A conversion error from
the operation that performed the rollback still fails its current owner.

Preserve #1286's mutation ordering. Check name/status and epoch overflow before
mutation. Under the root lock, advance the epoch and, only for a matching
operation, its authority before cursor pruning can mutate anything. If pruning
returns its ordinary pre-mutation error, restore both old values under that lock;
an unwind after mutation keeps the new epoch and matching authority. If later
restoration fails after the root lock is released, the new epoch remains, and
the matching operation's actual error is attributed to that epoch. Missing
savepoints and other pre-mutation failures do not adopt an epoch. Do not acquire
the root lock while holding the authority lock, and do not assume this fences
already-running effects or makes post-lock cleanup atomic.

Authored synchronized old-query/rollback-to and own-rollback controls cover the
same UUID and differing epochs, a stale operation performing another rollback,
missing/pre-mutation failure, and partial restoration failure. The supporting
tests include a real saved unique-key conflict whose restoration fails after
root state changes, and a synchronized later rollback before that old error
returns. Another authored test stages a private sequence and uses a scoped
test seam to cancel only after the actual cursor-prune branch advances
the epoch; setup's ambient controls are dropped before the request. The
observers and epoch accessor are implemented, and the bounded tests passed at `a59e7c65`.
Jev favored epoch comparison
(0.79), flagged stale re-failure (0.66), own-rollback adoption (0.67), and
partial-failure risk (0.42). Source and regressions decide correctness; receipts:
`target/1290-review/rollback-epoch-review-{request,response}.json`.

### Admission ordering correction required

Source review of the first candidate found that converted-query entry captured
authority using the transaction mutex before the existing nonblocking running-
query admission. When all slots are occupied, a request can therefore wait for
a cloned session's mutex instead of returning the existing admission-exhausted
error immediately. That candidate is not ready for integration. A synchronized
test must hold the real transaction mutex while occupying the query slot and
require the saturated request to finish with the original error before the mutex
is released, including a request whose cancellation token is already canceled.

Ordinary-path admission-before-capture is a candidate correction, but the
quarantined path currently runs before ordinary slot admission and can perform
its own rollback. Cancellation while waiting for an admitted operation's new
capture lock also needs an explicit outcome: returning before capture cannot
identify an active owner to fail, while waiting can delay cancellation. Do not
silently discard same-owner failure semantics or claim the quarantine rollback
is owner-safe. The selected partial ordering and interruption behavior are
described below; bounded tests in this section passed at `a59e7c65`.

Two additional authored controls specify existing recovery precedence. A
latched recovery error wins over both an occupied query slot and an already
canceled request, while an active matching transaction becomes Failed. A
separate synchronized test requires a future hook to observe the actual
healthy recovery check before admission; if recovery latches only afterward
while the slot is occupied, the original single-check path returns admission
exhausted. The hook and test passed at `a59e7c65`. Terminal
quarantined rollback still needs owner-safe retirement and its own race tests;
neither control proves that lifecycle.

For this explicitly partial increment, retain the original pre-admission
quarantine helper and its legacy completion unchanged; neither its rollback nor
the inner core's later quarantine check gains owner safety here. On the ordinary
path, keep the existing nonblocking query-slot admission before the new owner
capture. A saturated request returns the original admission error even if its
token is canceled, without waiting for the transaction mutex. The authority is
the transaction observed at the actual post-admission capture, not necessarily
the transaction present at API entry. A replacement before capture can therefore
be selected as the owner; protecting that earlier interval is a full #1290
obligation and this partial slice cannot claim it.

EXPLAIN must retain its existing statement-timeout snapshot before the new
post-admission authority capture. The first candidate instead read the setting
after that potentially blocked capture, so a concurrent `SET` could change the
deadline for a query already admitted. Authored synchronized controls use a
disabled global timeout, change the session setting while the actual capture is
contended, and observe the same controls delivered to the EXPLAIN converter in
both disabled-to-enabled and enabled-to-disabled directions. The current
candidate restores the original timeout-before-capture order; these tests passed
at `a59e7c65`. It does not claim to control other session-setting races.

For an admitted request whose root mutex is held, the selected candidate waits
for that mutex, captures its then-current owner and epoch, and then runs the
existing core's error, cancellation and deadline checks. If cancellation happens
during that wait, the response may wait until the mutex is released. For an
otherwise-admissible healthy statement, cancellation at its existing check
returns `QueryCanceled` and marks a matching current active owner Failed. The
core's earlier empty-user, quarantine, database-access and unsupported-SQL
errors still take precedence; cancellation remains before the deadline check.
Existing failure completion also locks the same transaction and
may delay response, but the earlier capture changes timing and may expose new
dependencies; preserve that limitation and prove the lock graph before shipping.
Do not return an interrupt without an owner and silently skip the existing
same-owner failure mark. A scoped test must observe actual capture contention
after successful slot admission, cancel while another thread holds the root
mutex, then release it and assert exact cancellation, same UUID and Failed
state. The capture itself tries the root lock and invokes the test-only observer
only on that actual failed attempt, immediately before its blocking fallback;
no separate preflight try-lock substitutes for capture. Compare deadline and
early error precedence separately.

Jev favored waiting rather than returning without owner attribution (1.0),
while flagging changed timing (0.84), replacement before capture (0.95) and a
possible new cyclic dependency (0.31). The request/response are in
`target/1290-design/capture-wait-{request,response}.json`; all are source-review
leads, not qualification. At that capture-wait increment, the unchanged quarantine path remained a
blocker to full #1290. The subsequent integrated carrier and owner-checked
retirement repair supplies that separately reviewed design.

COPY FROM binding first validates that the request names a table target. An
invalid query target already returns `COPY FROM STDIN requires a table target`
without acquiring the transaction mutex. The candidate must keep that check
before its new authority capture. For table targets, the existing subsequent
`parse_statement_for_session` first resolves `type_catalog`, which itself reads
the session transaction; capture immediately after table-target validation is
the selected bounded point, subject to further source and executable review of
all preflight/error ordering. The authored held-mutex test checks the actual
bind result before unlock, not the later pgwire error response. The full wire
and transaction-retirement paths remain outside this slice.

## Integrated admission error order

The integrated #1290 contract distinguishes entry quarantine from a stale
admitted operation. An already-quarantined entry or current-owner operation
returns storage-recovery-required (`58030`) under its existing quarantine path.
If recovery latches after admission and captured transaction A has already been
replaced by B, the admitted-owner check rejects A with `25000` before entering
the SQL core. That rejection must leave B healthy and retain its identity; it
does not clear the storage fence. A subsequent operation on B still returns
`58030`. This phase ordering is the selected source-backed Jev decision for the
observed late-latch test mismatch. Current-owner quarantine controls and all
three stale-entry routes passed in the default suite at `843500f8`; exact
subsequent mechanical changes and publication-head checks remain separately
identified in the qualification record.
