# Shared session-object mutation ownership

This selected #1290/#1279 repair complements the
[transaction authority contract](transaction-authority-midge-contract.md).
The regression controls are authored; executable qualification remains deferred
to the coherent runtime bundle.

SQL PREPARE, DECLARE, FETCH/MOVE, CLOSE and DEALLOCATE validate their operation's
captured transaction UUID, rollback epoch and generation at the actual shared
namespace or cursor-position mutation. A statement fork uses the captured
authority's real root, not its private transaction mutex; its projection owner
must still match that root. A detached fork without lineage cannot mutate shared
objects. A replaced operation returns `25000` without changing the newer owner's
objects. Current failed SQL blocks keep `25P02`.

The bounded critical section takes root then session objects. Heavy query
execution and prepared payload construction remain outside it. Final PREPARE
and DECLARE publication rechecks duplicate names and resource counts. DECLARE
receives root status from the guard instead of recursively acquiring the root.
Transaction retirement keeps its existing caller-under-root cleanup.

Protocol registration and close validate a scoped operation at their actual
shared-namespace write. Scoped registry writes run through the existing owned
blocking boundary so root contention does not block the async connection.
Unscoped connection cleanup preserves its current
wire-ID check and current-connection meaning. Named prepared statements retain
their connection lifetime; SQL WITH HOLD cursors retain their existing lifetime.
While COMMIT is sealed, a detached fork cannot enter shared-object mutation even
on the committing thread; only the lifecycle-owning committing root retains its
existing owned permission.

A lifecycle-owning operation may finish bounded shared-object publication after
its own recorded retirement when the actual root is still idle at exactly that
generation. This preserves an embedded WITH HOLD declaration whose modifying
CTE committed its own implicit transaction before the cursor is published. The
permission requires a captured, non-rejected same-root owner and its own
retirement receipt. An unrelated terminal command, subsequent transaction/ABA,
or detached fork does not receive it. This exception does not authorize staged
Data publication, mode changes or native sequence effects and never repeats
already committed effects.

DISCARD ALL validates its owner before any destruction and retains output
admission before resetting state. Its final reset takes sequence history then
the actual root, followed by objects and settings/search path. It clears sequence
history only when the guarded reset succeeds. DISCARD SEQUENCES uses the same
sequence-before-root order. Neither path acquires sequence history while already
holding root. Direct public APIs without an execution context retain their
existing current-session meaning. No guard encloses native storage I/O.
