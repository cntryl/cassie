# PostgreSQL blocking-boundary cleanup

The selected #1279 repair keeps an operation and its deferred schema cleanup in
the same started `spawn_blocking` worker. Native cleanup never runs on the async
connection thread and no independent cleanup task is spawned.

The worker catches unwinding from the operation, lets its stack unwind, then
checks the existing storage-recovery latch. When healthy it attempts the same
deferred cleanup before returning the operation's original result or resuming
the original panic payload. Cleanup errors keep their existing advisory
handling. The outer boundary preserves its join-failure mapping. This does not
make aborting panics recoverable or prove native storage quiescence.

Aborting an awaiting async task does not abandon cleanup in a started worker;
the worker retains its Cassie owner until cleanup returns. A queued worker that
never starts creates no new operation-owned work and performs no cleanup.
Existing durable cleanup debt remains for a later normal/startup attempt, and
the canceled join remains an error. No fallback cleanup worker is spawned.

Synchronized controls cover normal/error/panic operation results, recovery-latch
skip, abort of the awaiting task, and cancellation before worker start. They are
authored before implementation and passed in the default suite at `843500f8`.
Observed fixture repairs and later name-only changes retain their exact evidence
in the [qualification record](evidence/runtime-closeout-2026-11-17/README.md). This boundary alone does not
establish a global admission bound for all started COPY or blocking workers.

## Retained worker admission

Each configured listener's accepted connection owns its existing connection
semaphore permit through a resource-only Tokio task-local Arc. Every actual
blocking spawn reads and explicitly clones that resource on the async task before
spawning. The clone lives outside the complete operation-and-cleanup closure, so
panic, waiter abort, disconnect and listener shutdown cannot return the slot while
the started worker or its cleanup still runs. Queued cancellation drops the clone
without executing a new operation or cleanup. There is no extra query permit and
COPY WHERE retains its existing nested-query admission precedence.

The finite bound is per configured listener: at most one blocking continuation per
admitted connection, because the connection loop awaits its boundaries serially
and a canceled handshake exits. This is not a runtime-global bound across arbitrary
listener instances or direct external calls to the public `run_connection` helper
outside that listener admission scope. It does not claim cancellation, a time bound,
or quiescence of already admitted native/background work. Failure authority remains
an explicit operation field; the resource task-local does not carry transaction
identity.

The production spawn census is the common blocking boundary plus the three
protocol ownership/failure helpers in `transactions.rs`; all use the same explicit
resource-capturing spawn wrapper. Child Tokio tasks do not inherit this task-local
implicitly. Rejecting a connection runs no database blocking worker.
