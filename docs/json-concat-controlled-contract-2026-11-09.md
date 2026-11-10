# Controlled JSON concatenation candidate

The #809 increment is restricted to the existing `json_concat::concatenate`
dispatch and uses the checked raw scanner, object index and renderer when its
input and owner conditions are proved. Other inputs retain their legacy path.
The plain-object ordinary sentinel-key round-trip semantics is separate and remains incomplete.
Midge remains the published direct storage layer.

The existing function checks SQL NULL, then both operands' JSON carrier types,
then fully parses the left raw document and the right raw document, in that
order. When both roots are objects it parses all left object keys followed by
all right object keys, uses the last duplicate and makes right values win,
then formats keys by UTF-8 byte length and bytes while decoding/formatting
only the surviving values. Otherwise each array contributes its elements and
each non-array contributes one value; array inputs are parsed left then right
and output elements are formatted in that sequence. Invalid keys or values
that are reached earlier keep their precedence, including nested malformed
strings and numeric overflow; discarded raw values are not eagerly decoded.
Current typed JSON dispatch and the generic array-first dispatch must be
preserved, including Cassie's existing JSON extension behavior.

PostgreSQL 18.6 returns `{"a": 3, "b": 2}` for
`'{"a":1,"b":2}'::jsonb || '{"a":3}'::jsonb`, `[1, 2, 3]` for array plus
scalar, `[1, 2]` for two scalars, and `[{"x": 1}, 2]` for object plus array.
This is an oracle for JSONB, not evidence that PostgreSQL supports Cassie's
separate JSON overload or that the sentinel issue is solved.

Jev selected the scoped owned route with fallback (0.93), with precedence
(0.91), owner/cancellation/teardown (0.87), and overload/fallback (0.91)
leads. Receipt `target/assembly-review/json-concat-controlled-contract-{request,response}.json`.
Author differential probes for left/right syntax and key failures, escaped
duplicates and right winners, discarded and surviving malformed/overflow/NUL/
surrogate values, sentinel keys, arrays/scalars, SQL NULL and dispatch. Add
actual move/copy interruption and exact/below overlap/lifetime probes before
claiming the path; all remain unexecuted until the consolidated gate. Track
source/operand/output owners separately, keep fallback unchanged, and document
earlier control/admission errors, allocator transient excess, error ownership,
repeated scans and teardown limits. This increment does not qualify existing
stored data, all consumers, or the full #809 issue.
