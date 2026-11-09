# JSON containment resource qualification

This source increment belongs to [#809](https://github.com/cntryl/cassie/issues/809).
Its new executable probes remain pending the complete integrated query bundle
gate. Earlier passing operator semantics are baseline evidence.

Containment keeps its existing object, array, exact numeric and NULL semantics.
The private traversal uses an explicit stack whose frames each own a query-memory
reservation before copying raw values, parsing their immediate containers or
allocating a stack node. Child comparisons receive separate reservations. Both
normal completion and cancellation retire the linked frames iteratively.

Raw JSON parsing uses `serde_json`'s iterative raw-value decoder; it does not
inherit the ordinary value decoder's nesting limit. Charging each live frame
preserves deep raw JSON inputs within the configured memory budget without
introducing a new depth limit. Visited numeric comparisons separately admit
coefficient work before expanding short exponent tokens. Unvisited legacy JSON
numbers keep their existing lazy behavior.

Admission bounds also preserve the existing visited legacy numeric errors. A
range failure in the text-size estimator reserves the representable coefficient
bound and leaves the actual error to the existing parser. The parser skips
power-of-ten construction for a zero coefficient with a positive exponent;
successful values, scale and existing range checks are preserved.

Current statement controls reach row execution and constant evaluation. Checks
run at traversal transitions, scalar comparisons and result publication. The
new private probes cancel after an actual matching or nonmatching scalar
comparison, including a deep leaf with its parent workspaces still owned.
They also cover low/high memory budgets, exponent expansion, strict NULL,
direction reversal and both evaluator entry points.

Jev selected this private traversal and ownership design. An initial resource
gap probability of 0.38 prompted inspection of the exact dependency's raw decoder
and the per-frame design. Source review returned semantic gap 0.20, ownership
gap 0.24 and cancellation gap 0.35; the latter prompted the deep-leaf cleanup and
entry-point probes. These judgments do not replace executable qualification.

This increment concerns Boolean containment workspace. Other JSON operators,
scalar outputs, JSONPath and expansion ownership retain their own qualification
obligations; this record does not close #809 or #810.

## Visited string scalar comparison candidate

The existing controlled walk calls `serde_json::from_str::<String>` on the
candidate followed by the required value, then compares the two decoded
Strings. Those individual copies and the equality walk are not interruptible
or separately owned by the admitted parent pair. Jev selected checked decoding
of those two visited raw strings and checked bytewise equality when controls
exist (0.96); the uncontrolled route stays unchanged. Receipt:
`target/assembly-review/json-containment-string-contract-{request,response}.json`.
Decode candidate before required, retain both independent owners until
comparison finishes, preserve existing bare serde error text and position,
propagate actual control errors, and keep exact decoded UTF-8 byte equality.
Additional admission and cancellation can precede old errors. Differential
escaped strings, Unicode, lone surrogates and error order, exact/below overlap,
and synchronized actual-copy/compare interruption probes are authored before
implementation and deferred until the combined gate. The parent raw pair,
map parsing/comparison, numeric work, diagnostic allocation, allocator
transient excess and recursive/third-party teardown are not qualified by this
increment; Midge and the open private-sentinel representation issue are unchanged.
PostgreSQL 18.6 reports true for `"a"` versus `"\u0061"`, true for an
array containing `"é"` versus `"\u00e9"`, and false for `"éclair"`
versus `"éclai"` under JSONB `@>`.

## Visited finite numeric equality candidate

For validated JSON number tokens, use the checked `raw_number` formatter in
candidate-then-required order, keep both formatted text owners alive, and
compare their canonical decimal text after checking and removing only trailing
fractional zeros and an empty decimal point. This preserves zero sign removal,
scale-independent equality and representable numeric range; malformed and
out-of-range inputs retain their original parse/error ordering. The legacy
no-controls route remains intact. Jev selected this bounded route (0.92), with
parity/boundary (0.92), error/resource ordering (0.71) and ownership/
interruption (0.89) leads. Receipt:
`target/assembly-review/json-containment-number-contract-{request,response}.json`.
Authored but unexecuted probes cover scaled equality, signed zero, exponents,
range and competing errors, checked arithmetic/copy/compare interruption and
exact/below admission. Both formatted Strings overlap the parent pair and can
cause earlier resource or cancellation errors; allocation transient excess,
parent/container parsing, diagnostics, numeric formatter limits and teardown
remain open. This does not qualify sentinel storage or complete #809.
PostgreSQL 18.6 reports true for `1.000 @> 1`, true for `-0.000 @> 0e4`,
false for `1.0001 @> 1.001`, and true for `[1e2] @> 100` with JSONB operands.

## Root object containment candidate

When controls exist and both root values are objects, retain both raw sources,
validate complete candidate then required syntax, decode all candidate keys
then all required keys, and order their duplicate-last entries by checked UTF-8
byte lexicographic order. This is the legacy `BTreeMap` iteration order, not
the length-first JSONB display order. Merge matching keys in required-key order;
invoke the existing child walk with `root=false` and the same scalar observer,
short-circuiting on missing keys or failed children. The `<@` caller must
continue reversing candidate and required. Other roots and calls without
controls retain the existing path.

Jev selected the scoped route (0.92), with key order/duplicates/errors (0.90),
root handling/observer/reversal (0.67), and source/key/sort/child ownership
(0.92) as leads. Receipt:
`target/assembly-review/json-containment-root-object-contract-{request,response}.json`.
Author tests for length-vs-lexical error precedence, duplicate last-wins,
escaped keys, candidate-before-required errors, scalar observers, nested
array-versus-scalar behavior, both directions, exact/below admission, and
actual sort/comparison cancellation. All probes remain unexecuted until the
combined gate. Newly admitted key directories and sorting can report control
errors earlier; nested container parsing, general raw pairs, diagnostics,
allocator transients, teardown and sentinel/storage migration remain open.
PostgreSQL 18.6 yields false for `{"a":[1]} @> {"a":1}`, true for
`[1] @> 1`, true for `{"a":1,"a":2} @> {"a":2}`, and true for
`{"a":[1,2]} @> {"a":[2]}` with JSONB operands.

## Pair failure lifetime correction

The existing pair declares both raw boxes before their reservation, but
destructuring them into separate locals declares the reservation last. Rust
drops those locals in reverse order on a scalar or parser error, releasing the
lease while the boxes can still be live. Keep the Pair structurally intact
through fallible work; when a root array comparison moves its required scalar
into the temporary array, that array must drop before the remaining Pair on
error. Transfer the reservation into a successful frame only after those
operations. A test-only hook cancels after the actual raw pair was built;
other probes cover required-scalar array success and error cleanup. The hook
checks release, not the exact timing of a String destructor. No production
interruption point is added. Other parser, transition, memory estimate and
nested-container limits remain open; these probes are unexecuted.

## Nested object containment candidate

For controlled nested object pairs, detach each checked object index into
owned decoded keys, raw-source byte ranges, stable lexical duplicate-last
indices, and their reservations. Move both original RawValue boxes and both
detached indexes into a Frame-owned object pair; no field borrows the source it
is stored beside. Merge required keys lexically inside the existing iterative
walker, passing matched children through its regular pair/equality path. Keep
the outer Pair reservation alive while the builder owns raw inputs and until
the Frame takes that reservation. Preserve candidate-then-required key parse,
the observer, short-circuit order, and the no-controls legacy route.

Jev selected this bounded design (0.89), with parse/order/observer (0.58),
source/index/Frame failure ownership (0.38), and controls/teardown (0.77)
leads. Receipt:
`target/assembly-review/json-containment-nested-object-contract-{request,response}.json`.
Unexecuted probes cover nested/deep objects, duplicate and escaped keys,
lexical and malformed-key precedence, both directions, synchronized actual
index work cancellation, and exact/below peak admission. Extra index storage
can change budget/cancellation precedence. Nested arrays, general raw copies,
diagnostic allocations, allocator transients and teardown remain separate
boundaries; storage sentinel behavior and full #809 remain open.
PostgreSQL 18.6 reports true for `[{"a":1,"a":2}] @> [{"a":2}]`, false for
`[{"a":[1]}] @> [{"a":1}]`, and true for matching nested `outer.k` objects,
with JSONB operands.

## Controlled array containment candidate

With controls, an iterative array Frame can own both existing raw sources and
admitted directories of child byte ranges, in place of constructing a boxed
RawValue for every element. Build the required array directory first and then
the candidate directory, preserving the current parse and error order. A root
scalar requirement is one raw span; it is not generalized to nested arrays.
Advance required elements and retry candidate elements in the legacy order,
forward the same scalar observer, and pass each selected pair to the existing
child walk. Without controls, keep the legacy arrays and parser.

Jev selected the bounded design (0.67), with parse/retry/root/observer (0.77),
partial owners/cancellation (0.49) and repeated search/diagnostics/teardown
(0.86) leads. Receipt:
`target/assembly-review/json-containment-array-contract-{request,response}.json`.
Test-first probes cover required-first parsing, duplicate and missing elements,
array/scalar root versus nested behavior, both directions, invalid surviving
values, actual range-directory movement and exact/below-budget overlap. These
probes are unexecuted. Directory admission can surface resource or cancellation
errors earlier; the existing repeated candidate search can remain quadratic.
General raw parsing, diagnostic allocation, source/allocator transients,
teardown and sentinel/storage migration remain outside this increment.
PostgreSQL 18.6 yields true for `[1] @> [1,1]`, true for `[1,2] @> [2,1]`,
false for `[1,2] @> 3`, and true for `[] @> []` with JSONB operands.
