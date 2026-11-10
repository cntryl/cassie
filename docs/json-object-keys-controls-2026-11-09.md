# Controlled JSON object-key projection

For a String-backed `json_object_keys` or `jsonb_object_keys` call with both
statement controls and an expression owner, retain the existing arity, SQL NULL
and inspection scratch order, parse with the owned Value-mode arena and visit
its object keys in the existing UTF-8 lexical order. Pre-admit the result Value
vector and each copied key, retaining source arena and scratch until the copied
Value array and its reservation are handed to the same-tracker expression
owner. A semantic parse failure falls back to `document()`; non-objects preserve
the existing error. Value-backed, ownerless, general-owner and no-controls
calls retain their existing route, as do all private-sentinel interpretations.

For the existing single bare-name SELECT-list expansion only, activate the
projection owner for `json_object_keys` or `jsonb_object_keys`. Function names
are matched case-insensitively as in the expansion route. The builtin dispatcher
checks the static JSON builtin before user-defined fallback for these bare
names, so this does not select an overridden implementation. Qualified names,
nested scalar calls and multi-item projections retain their previous selection.
The source row and its owner remain live while existing expansion admits each
output row. Expansion's per-key clone and physical row capacity, earlier
argument clones, generic nested scalar conversion, fallback parsing, allocator
transients and teardown remain unqualified; this is not a whole-query or
sentinel/storage completion claim.

Jev selected checked construction (0.93; lexical/duplicate/error 0.65,
partial ownership 0.62, caller scope 0.79) and the bare projection route
(0.76; routing 0.87, handoff 0.80, remaining limits 0.71). Receipts:
`target/assembly-review/json-object-keys-arena-contract-{request,response}.json`
and `target/assembly-review/json-object-keys-projection-contract-{request,response}.json`.
Additional checked work/admission may precede legacy errors and retains output
charges longer. Differential, cancellation, tracker, budget and projection
handoff probes are authored before implementation and remain unexecuted.
