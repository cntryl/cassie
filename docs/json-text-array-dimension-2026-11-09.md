# Checked single-dimension JSON operand paths

The shared owned TEXT[] operand parser may select a PostgreSQL literal with one
explicit dimension, such as `[0:1]={a,b}`, after checking signed INT4 bounds,
the i64 length calculation and a valid flat brace body. It holds the existing
owned strings and directory through deletion, extraction or existence. Other
forms, including multiple dimensions, malformed or out-of-range bounds, invalid
bodies and declared/nonempty actual shape mismatches, retain the legacy route
for its diagnostics. This does not change the separate JSON-array policy.

Match the current parser's Unicode whitespace trimming inside bounds and
ASCII-only separator whitespace; an empty brace body follows its existing
exception that does not compare the declared shape. Check and observe actual
prefix work so cancellation is bounded; never introduce a size or depth cap.
The prefix recognizer can also scan leading whitespace or an initial JSON-array
number before falling back. The legacy parser may be invoked after the checked
attempt, and semantic/error fallback remains unbounded. Additional checks/admission can take precedence
over later legacy work; error String ownership, nested forms, allocator
transients and teardown remain unqualified.

Jev selected the scoped candidate (0.81), with grammar/bounds/empty precedence
(0.70) and ownership/cancellation (0.49) leads. Receipt:
`target/assembly-review/json-text-array-dimension-contract-{request,response}.json`.
Differential tests for signed/extreme bounds, Unicode/ASCII whitespace, empty,
shape/fallback, actual prefix/copy cancellation, exact/below budget and caller
handoffs are authored before implementation and remain unexecuted until the
consolidated gate. The private-sentinel/storage issue remains open.

A PostgreSQL 18.6 oracle returned `{"b": 2}`, `1`, and `true` respectively
for JSONB deletion by `[0:0]={a}`, extraction by `[7:8]={a,b}`, and `?&`
with `[5:6]={a,b}` (each explicitly cast to `text[]`). This witnesses
lower-bound-independent element use, not the whole literal grammar.
