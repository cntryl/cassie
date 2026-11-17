# Transaction-owner mutation inventory

This is the finite source inventory for #1290 and #1279. It describes candidate
source under review; the focused controls and complete gates have not run.
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

## Remaining integration and review obligations

- Fork schema contexts currently share a root Arc. The selected repair is an
  execution-local owned context clone retaining its descriptor lease, Weak
  Midge link and same-query known-payload reservation. Root attachment must
  validate lineage under root-to-context lock order.
- Per-portal owner metadata and guarded Bind publication are source-reviewed
  in a separate coordinator increment. Actual Execute/Sync adoption and
  cleanup wiring must be reviewed with this candidate; a generation-only
  clear is not the selected rule. Portal lifetime uses transaction UUID and
  generation, while operation admission also checks rollback epoch.
- Shared SQL cursor/prepared/discard leaf mutations are being checked against
  the same explicit lineage contract; root transaction guards alone are not
  proof of ownership for a separate shared Arc.
- Preserve the actual staged TRUNCATE/identity recovery assertions and add a
  detached A-to-B restart-boundary control. The former compensator list is
  gone; do not recreate it or infer a native/background quiescence guarantee.

All new regressions remain authored/not run until the assembler owns the
frozen integrated bundle. The broader published-Midge/native/background
qualification blocker in #1188 remains separate; this inventory adds no engine
capability and does not close that issue.
