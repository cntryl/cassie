# Multidimensional key-existence operands

`?|` and `?&` inspect TEXT[] elements in row-major order irrespective of array
lower bounds. The existing native `SqlArray` route already iterates flattened
elements. For a string-backed PostgreSQL literal, the old existence fallback
used `parse_text_array(...).value`: nested brace arrays become nested JSON arrays,
whose top-level elements are then skipped as non-string keys. Consequently a
present nested key could be missed and `?&` could incorrectly succeed as though
there were no keys. The same parsed result's `flat_values` already contains the
validated row-major elements; selecting it for this existence-only fallback
preserves complete parsing, rectangular/dimension validation, text/null
conversion and the document-before-operand error order. JSON-array input's
`flat_values` is its existing normalized array. This does not change deletion
or extraction paths.

A PostgreSQL 18.6 oracle returned true, false, true for `?| '{{z,b}}'`,
`?& '{{a,z}}'`, `?& '{{a,b}}'` on `{"a":1,"b":2}`; the first two
results also held with `[0:0][3:4]={{...}}`. NULL keys were ignored and a
ragged literal returned 22P02. Direct native, one-dimensional, JSON-array,
controlled-fallback, lower-bound, malformed and SQL probes were authored before
implementation and are unexecuted. Reviewer source and oracle evidence make the
choice mechanical. The fallback still uses the existing unbounded/unowned array
parser; no cancellation, allocator or broader #809 qualification is claimed.
Ordinary private-sentinel key semantics remain a separate #809 obligation.
