# DISCARD sequence history and restart rollback (#1304)

`DISCARD SEQUENCES` forgets the session's `currval` and `lastval` history; it
does not discard a transaction's pending `TRUNCATE ... RESTART IDENTITY`
restoration. Clear the history fields while retaining restart compensators
when the session transaction is active or failed, including implicit blocks.
When it is idle, clear orphaned compensators as the previous implementation
did. The same history operation is used by `DISCARD ALL`;
its existing active-transaction rejection and other session resets stay in
force. No new storage or durable-format behavior is selected.

The history/compensator decision and transaction status observation occur
atomically while holding the session sequence-state mutex followed by the
transaction mutex. This order matches `last_sequence_value`, which holds the
sequence mutex while resolving catalog state through the transaction mutex;
the reverse order can deadlock with that existing path. Extract any discarded
compensators while locked, but drop them after both guards are released; never
invoke a restorer or perform native I/O inside these guards. Source review and
Jev favored the sequence-first order but did not prove the entire transitive
lock graph; supervised lastval and BEGIN progress tests passed locally at
`b12b994a`.

Detached statement forks share the sequence state but have a separate
transaction mutex. Their private status is not root transaction authority. A
detached caller therefore conservatively retains compensators regardless of
its own status; only a lifecycle-owning session can clear orphan callbacks on
an Idle observation. A focused forced-Idle fork test checks root callback
preservation; it does not assert that SQL DISCARD reaches a detached fork.

This decision protects only callbacks present at that observation. An old
effect may still register a callback afterward: admission and retirement of
in-flight effects remain open under #1302. Tests use a directly registered
orphan callback to isolate idle DISCARD behavior; they do not claim the later
registration race is fixed.

Tests must use a committed, non-private identity sequence, distinguish its real
native pre-restart and restarted cursors, and check the row after rollback and
commit. Verify that `currval` and `lastval` are forgotten after the terminal
operation, along with ordinary outside-transaction DISCARD, normal rollback and
`DISCARD ALL` rejection. Avoid deliberate history errors before a commit
control: an error inside a block itself changes transaction state. The new
tests were not run against an instrumented baseline, so no baseline RED was
observed.

This bounded fix does not solve the observed cross-session sequence overwrite
or the cloned-session admission and retirement race under #1302/#1290. The
parent #1301 has merged. The full locked suite passed at `b12b994a`; affected
sequence tests, strict Clippy, formatting, test policy and inventory passed at
`bb55d527` after a test-only typed-assertion correction. Final-head hosted checks
remain pending.
