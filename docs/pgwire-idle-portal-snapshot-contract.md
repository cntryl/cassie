# Conditional pgwire portal cleanup (#1300)

This child of #1297 / PR #1299 covers four conditional idle portal-clear
choices: `transactions::clear_idle_portals`, the extended error path's recovery
cleanup, extended Sync, and the Sync-wait handler. The parent merged as
`fdb9038b122d0fa73ef815195312cf4ae8c54591`. The full ordered local gate passed
at `a5ca12c9`; final-head hosted checks are pending. These choices derive from one
`transaction_status_snapshot`, clearing only if that point-in-time status is
Idle; release the transaction mutex before clearing portals or awaiting I/O.

A Failed-to-InTransaction savepoint recovery between the prior separate
not-active and not-failed reads can otherwise select a cleanup for an Idle
state that was never observed. The fix does not prevent the session changing
after the snapshot, and does not establish portal ownership or atomic cleanup.
The Sync-wait `invalidate_portals_on_sync` flag still clears unconditionally
and short-circuits the idle decision. Implicit-transaction cleanup, error
handling, generation-dependent portal clears and split status reads used for
implicit-start decisions are unchanged and remain #1290 obligations. No
complete wire correctness or replayable SQL claim follows.

Author synchronized tests using actual retained portals and a hook after the
real status snapshot with the transaction lock released. Restore a Failed block
to InTransaction via ROLLBACK TO while paused, then verify the Failed snapshot
retains the portal; also check ordinary Idle, InTransaction and Failed outcomes,
real Sync/recovery paths and the unconditional Sync-wait invalidation branch.
The nine portal controls passed in the locked suite at `a5ca12c9`. An externally
owned watchdog bounds blocked asynchronous execution. A baseline RED requires
separate instrumentation between the old reads and an actual execution; it was
not observed.
