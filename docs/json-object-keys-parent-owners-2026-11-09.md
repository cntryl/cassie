# JSON object-key expansion source parents

When an expanded key row shares the input row's `data_types` Arc, retain that
input row's existing query and operator owner Arcs on the output row. Those
parents are retained, not absorbed, so a foreign-tracker source remains owned
by its own tracker. Rows with no shared type Arc do not retain unrelated source
owners. The source can have no owner; this change does not establish that every
producer admitted type storage.

Before final output attachment, hold rows and the output reservation in a
value-first guard, admit the shared reservation Arc estimate, and attach the
output reservation without replacing the preserved parents. Rows without
parents may retain it directly; rows with parents use the existing fallible
operator-link path. If attachment or interruption fails, all rows drop before
the shared output reservation. The last returned row clone keeps its attached
parents and output reservation. Zero rows retire the reservation immediately.

Source evidence: `BatchRow::owned_body_bytes` includes the data-type Arc,
capacity and nested type heap, and the selected projection handoff admits that
estimate and attaches it to rows. Scan and other producers do not universally
prove such admission, so parent retention is conservative lifetime coverage,
not physical type-capacity qualification. `attach_operator_memory` preserves
previous operator and query origins via Arc parents; it does not absorb those
foreign reservations. The new output lease itself must share the active
tracker. Arc-owned reservation/header deallocation windows, metadata allocator
excess, lookup caches, chunks and backing-capacity accounting remain open.
New Arc/link admission and interruption can precede old results, and retaining
source parents can increase budget pressure.

Jev selected the parent-plus-checked-vector direction (0.59); this separate
parent-first checkpoint preserves that dependency. Receipt:
`target/assembly-review/json-projection-parent-evidence-{request,response}.json`.
Authored query-only/operator-only/both/mixed-tracker, typed/untyped, empty,
partial attachment/cancellation, exact/below and last-clone probes remain
unexecuted until the consolidated gate.
