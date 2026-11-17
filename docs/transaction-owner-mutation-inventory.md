# Transaction-owner mutation inventory

This is the finite source inventory for #1290 and #1279. The default suite and
34 feature controls passed at `843500f8`. Later mechanical lint and test-name
changes, their equivalence proofs and affected gates are recorded in the
[qualification record](evidence/runtime-closeout-2026-11-17/README.md); publication-head hosted results remain a separate check.
The selected contract is in
[Replayable SQL transactions](sql-replayable-transaction-contract.md).

| Mutation family | Source boundary | Ownership rule |
| --- | --- | --- |
| BEGIN and implicit origin/promotion | `session/transaction_control.rs` | Validate captured UUID, epoch and generation under the mutation lock; a deferred own start may adopt. Own terminal retirement is recorded for AND CHAIN. |
| COMMIT and full ROLLBACK | `query_transactions.rs`, `session/transaction_control.rs` | Seal, policy capture, retirement and error cleanup use the same admitted owner. Cleanup cannot retire a replacement. |
| Savepoint creation, restoration and release | `session/savepoints.rs` | Validate before mutation; own restoration adopts its new epoch. Restore settings/objects before unlocking; an actual key-restoration error unlocks before the error observer and completion. |
| First-query and transaction characteristics | `session/transaction_control.rs` | The owner check and `has_query`, isolation or read-only mutation share the root lock. |
| Row staging, renames and conflict intents | `session.rs`, `session/statement_batch.rs` | Check root ownership at fork creation, staging and publication. Detached state remains private; stale internal void bookkeeping does not mutate the replacement. |
| Projection declaration and effect history | `session/projection_write.rs` | Validate under the root lock before changing declaration/history; sequence strictness is admitted before native submission. |
| Catalog-overlay creation and cached read views | `session/catalog_overlay.rs` | Validate before installing mutable root aliases or cached compositions. The storage-overlay getter is fallible and validates before returning its stable Arc. |
| Repeatable-read initialization and fence transfer | `session/transaction_snapshot.rs` | Validate before installing a captured read owner or taking its publication fence. Existing generation/isolation checks remain. |
| SQL setting leaves | `session.rs`, `session/setting_control.rs` | Validate at the actual root-to-settings/search_path write, without recursively locking the dispatcher. Direct unscoped public setters keep current-session semantics. |
| SET LOCAL bookkeeping and write | `session/transaction_control.rs` | Retain one owner-checked root guard through both the restoration history and the actual in-memory settings change. |
| Recovery/quarantine | `storage_recovery.rs`, query lifecycle/worker entry points | Capture at the recovery origin before terminal cleanup/conversion; carry that same owner through nested helpers. Recovery retains precedence over ordinary slot admission. |
| Wire failure and output completion | `connection/transactions.rs`, simple/extended/COPY wrappers | The execution-local deferred owner is captured after admission and reused through completion; a rejected unowned query never recaptures a later transaction. |
| Retained COPY authority | `connection/copy/binding.rs`, `session/failure_authority.rs` | Inline binding B-S and shared owner payload are separately charged; the payload reservation lives inside the shared owner until the last actual clone retires, including an aborted awaiting task. |

The lock order is root transaction, then operation owner and the relevant
in-memory session/settings/catalog bookkeeping. No added guard spans native
storage I/O. Admission of an independent sequence effect can precede COMMIT;
that admission has already made Data strict before releasing the root lock.
It does not cancel, undo or bound an admitted native update.

## Implemented mutation and lifetime rules

- Fork schema contexts use the selected execution-local owned clone: retain
  catalog/descriptor lease references and the Weak Midge link, while owning a
  separate default-database String. Admit wrapper payload, known Arc counters
  and actual String bytes before cloning, and retain their memory guard with
  that same Arc until its last clone retires. Root attachment validates lineage
  under root-to-context lock order; fork attachment never writes a later root
  context. Jev selected this isolation over shared-guard-only at 1.0. This is a
  metadata lifetime owner, not another storage abstraction or an allocator
  metadata claim. Source and executable qualification remain separate.
- Per-portal owner metadata and guarded Bind publication are integrated with
  Execute/Sync adoption and owner-checked cleanup. A generation-only clear is
  not the selected rule. Portal lifetime uses transaction UUID and
  generation, while operation admission also checks rollback epoch.
- Shared SQL cursor/prepared/discard leaf mutations validate the same explicit
  lineage contract at their actual shared leaf; a root transaction guard alone
  does not establish ownership of a separate shared Arc.
- Staged TRUNCATE/identity recovery and detached A-to-B restart-boundary
  controls passed in the default suite. The former compensator list is gone;
  this does not establish native/background quiescence.

The qualification record preserves authored-before-code versus later wrapper
coverage, the absence of an instrumented baseline RED, observed failed gate
attempts, and subsequent passes with their exact source identities. The broader published-Midge/native/background
qualification blocker in #1188 remains separate; this inventory adds no engine
capability and does not close that issue.

## Transaction clock

Transaction clock publication validates the operation's actual root UUID, rollback
epoch and generation, then retains that root mutex through the shared clock read
or first-write (root→clock order). Statement forks use their captured actual-root
lineage, never their private transaction mutex. A stale actor returns 25000 before
clock mutation; with_statement_text propagates the private fallible installation.
This runs after existing query admission and adds no early capture or native I/O.
An exact lifecycle-root own-retirement receipt may install an Idle temporal view,
using statement_start without reading or writing transaction_clock; forks and
unrelated/ABA retirement do not gain this exception. The receipt grants no active
clock, staged-write, mode or settings mutation authority. The six synchronized
root/fork A→B, current first-clock, Idle, own-retirement and supported standalone
nested CALL controls passed in the default suite at `843500f8`.
