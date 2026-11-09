# Owned text-array operands for JSON deletion

This record covers the string-backed TEXT[] operands still on the legacy path
in #809. It does not claim issue completion. The source baseline is
`0046e943b5ba7acfeeab1894cf835ce5c83ab7c4`; all new probes remain unexecuted.

The existing `array::parse_text_array` accepts PostgreSQL brace literals with
optional dimensions or a JSON array when no dimension prefix is present. It
validates syntax, nesting, rectangularity and declared dimensions before
returning normalized TEXT/null elements. JSON-array input uses serde's Value
semantics, including its first-key private sentinels; deletion's raw-value
parser is not interchangeable. Error SQLSTATE, message and malformed-before-
type ordering remain observable. The existing deletion path then validates
the resulting one-dimensional strings/NULLs before traversing the document.

A focused Jev choice selected a test-first owned parser developed incrementally
(0.97), with grammar/dimensions/NULL/sentinel (0.87), error-order (0.86),
ownership (0.66), and allocator/diagnostic/teardown (0.89) leads. Receipt:
`target/assembly-review/json-text-array-owner-contract-{request,response}.json`.
Each admitted result String must carry its reservation through an owned path
directory; iterative frame, dimension and result directories need checked
growth and correct failure drop order. No estimated 192× envelope or detached
String may stand in for a proven transfer. Full shape validation precedes
element conversion, and no new depth or size cap is selected.

Probes must compare the old parser for brace/dimension/escaping/whitespace,
NULL, rectangularity, malformed-before-type, JSON Value sentinels and errors;
exercise actual scan, buffer and directory moves under cancellation; measure
exact and one-below budget overlap; and keep the path owner through deletion's
last use. Unproved forms stay on the old route. Diagnostic delivery, transient
allocator excess, recursive teardown and generic consumer handoffs remain
pending until independently proved. Interim source review does not replace
the consolidated build, test and clippy gate.

The controlled deletion handoff covers flat PostgreSQL brace literals without
a dimension prefix; nested/dimensioned and JSON-array inputs retain the old
route, including its existing unowned allocations.
It checks the whole input for nested brace syntax before constructing a path,
then parses quoted/unquoted elements with owned string and directory buffers.
That preflight and the new admission may surface cancellation or a resource
error before an otherwise later syntax error. This is an explicit precedence
change for the controlled route, not a new length/depth limit. The result's directory
and all transferred String capacities remain reserved through its last use.
The accepted path owner remains alive while deletion evaluates, and the
result's independent reservation is transferred to the expression owner.
An interruption check occurs after parsing and before document traversal, so
cancellation may precede a document-shape or rendering error at that boundary.
This does not qualify consumers without a deletion owner, the fallback forms,
diagnostic delivery, allocator transients or unexecuted probes.
