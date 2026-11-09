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
