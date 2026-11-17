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
authored before implementation and remain unexecuted until the coherent runtime
gate. This boundary alone does not establish the separate global admission
bound for all started COPY or blocking workers.
