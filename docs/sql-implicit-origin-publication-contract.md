# Atomic implicit transaction origin

Issue [#1280](https://github.com/cntryl/cassie/issues/1280) makes the origin of a newly started implicit session
transaction visible atomically with its status and generation. Another clone of
the same session must not observe an active transaction as explicit merely
because implicit initialization has not finished. This fixes the origin
publication boundary required for future transaction-local policy; it does not
implement replayable SQL mode or qualify #1279.

Publish the initialized origin, transaction status and generation under the
existing session transaction mutex. An explicit `BEGIN` still creates an explicit
transaction, and promotion of an already implicit transaction to explicit keeps
its existing characteristics and generation rules. No query executor, data
snapshot, catalog-publication lock, connection-wide execution mutex, Midge
format or durability policy is introduced by this prerequisite.

A deterministic regression pauses initialization at the origin publication
boundary and reads from a second clone of the same session. Its test-only seam
is placed between the two writes in the old implementation and after the single
atomic publication in the replacement, with no session mutex held during the
pause. The assertion observes the state while the initiating call is paused, not
only after it returns. Preserve implicit-to-explicit promotion and ordinary
explicit BEGIN as separate controls. The test seam has no production build
overhead. Newly authored regressions are not observed RED until executed.

This change does not prove admission across in-flight statement forks, COPY
streams, independently durable sequence effects, cancellation or COMMIT. Those
remain explicit #1279 work before any weaker replayable policy can be enabled.
