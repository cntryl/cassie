# Aggregate cancellation fixture scope

Issue #954 requires an instance-scoped retained-input boundary. The previous
diagnostic hook installed a thread-local pause, so any subsequently created
query handle on that thread could take it, including a handle from another
Cassie instance or an unrelated backend. That behavior could make a wire
fixture synchronize with the wrong request.

The existing doc-hidden hook now takes the Cassie instance and the target
pgwire backend's process ID and secret. The pending pause belongs to that
instance's backend registry, is claimed only by a new query on that backend,
and follows the query handle's clones once claimed. Dropping the guard removes
only its own unclaimed pause; disconnecting a backend removes its pending
entry. Unit-only direct attachment supports operator tests. This changes the
signature of an existing public, doc-hidden diagnostic function and can require
updates for downstream Rust callers of that function; it adds no new public
pause function. The existing sleep and regex hooks retain their own scopes.
An aggregate pause and a sleep or regex pause may coexist on one query handle;
each retains a separate one-shot owner and reaches only its own boundary.

The authored probes cover two instances and two backends on one executor,
an intervening bare handle, invalid and duplicate identities, guard drop,
disconnect, and real CancelRequest with no output and recovery. They have not
been run on this candidate. Additional probes cover coexistence with a sleep
pause, cloned-handle one-shot behavior, and an old guard dropped after a new
pause is armed. The rest of #954, including its full ordered gate
and timing comparison, remains open.
