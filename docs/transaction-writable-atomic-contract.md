# Atomic transaction writable check

Status: selected #1292 contract, not yet implemented or qualified. The child is
based on the #1291 integrated head `5a1bdbca4e2cb56b5d3099248521ef370e2332d6`
while that prerequisite is qualifying; rebase and review after its merge.
This is a bounded internal failure-transition fix, not completion of #1290's
query/pgwire error ownership or #1279's effect admission and durability mode.

For an active transaction, the writable check reads that transaction's read-only
mode and, when a write is forbidden, changes that same transaction to Failed
under one session transaction mutex acquisition. Return the existing `25006`
read-only error and message. A read-write transaction remains unchanged and the
check succeeds. An already-Failed transaction retains its status: a stored
read-only mode still returns `25006`, while read-write returns success from this
helper; callers separately enforce their existing failed-block rules. Implicit
transactions follow the same mode rules.

For Idle, release the transaction mutex before reading the session's default
read-only setting, preserving the existing settings/transaction lock order.
Return the same read-only error or success from that setting; do not subsequently
mark a transaction that another clone began while the default was read. Idle
status and the default setting are separate observations, not an atomic combined
snapshot. This does not guarantee that an old operation first reaching the
helper after B begins uses its original owner, that a successful check fences a
later state change, or that outer query/pgwire error handling cannot separately
fail B. Those user-visible transaction ownership races remain #1290/#1279.

At baseline `5a1bdbca4e2cb56b5d3099248521ef370e2332d6`, the helper first calls
`is_transaction_read_only`, then separately `mark_transaction_failed`. A clone
can replace read-only A with read-write B between those calls; the old check can
then fail B. The deterministic test observes the helper's decision after its
mutex has been released, replaces A with B, and checks old `25006`, A's prior
Failed state, and healthy B. The Idle/default-on control begins read-write B
after the default observation and checks the same boundary. A bounded channel
and joined worker prevent an accidentally held session mutex from wedging the
callback. Instrumented baseline probes would place the same callback between
its old check and mark; no baseline RED is claimed until executed.

Controls cover active read-only, read-write, already-Failed modes, implicit
read-only, and Idle defaults on/off, preserving their current outputs. Jev selected
the atomic active transition 1.0 and identified the remaining idle/default
observation and outer-wrapper races at 0.93 and 0.95; independent source/Jev
review confirmed the scope. Receipts are in the preserved #1290 design worktree
under `target/1290-design/readonly-atomic-{request,response}.json` and the
review worktree under `target/1290-review/writable-atomic-{request,response}.json`.
