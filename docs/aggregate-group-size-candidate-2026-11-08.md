# Aggregate serialized-size counter candidate

This is a local, test-only candidate; no production aggregate caller is wired.
The current callers require a separate reviewed bundle that keeps group values,
maps and their reservations correctly ordered through every new fallible
counting, insertion, merge and output path. Do not integrate an unused counter
or enable these fallible calls before those owner prerequisites are resolved.

The candidate counts the existing derived serde representation of aggregate
entry slices. It covers the currently defined `Value` variants: scalar tags,
UTF-8/escaped strings, compact JSON values, vectors, SQL arrays, fixed-width
typed values, NUMERIC and Bit/VarBit. Nested arrays use borrowed, admitted
iterative frames. Numeric text uses the separately source-reviewed owned
formatter; its worst-case conversion is quadratic, and maximum-width CPU
performance remains unqualified. Bit counting uses stored bytes, including
noncanonical deserialized padding. The generic serde fallback remains for
unproved future shapes, and scalar serialization errors retain the old zero
result. No new input, array-depth or numeric precision limit is introduced.

Unlike the former untracked output Vec, the candidate checks interruption and
can admit frame or numeric workspace before completing the count. It can report
cancellation or a query-budget failure earlier, including while determining a
later generic fallback. Output and stack allocations reject allocator capacity
greater than admitted, but temporary allocator excess before rejection remains
unproved. Borrowed input owners must outlive counting; this candidate does not
qualify input construction or destruction, map hashing, aggregate transitions,
output cloning, caller handoffs or transport.

The selection and source leads are recorded in the coordinator worktree's
`target/assembly-review/arena-linkage-and-group-*`, `aggregate-json-size-*`,
`aggregate-vector-size-*`, `aggregate-array-size-*`,
`aggregate-fixed-typed-size-*`, and `aggregate-dynamic-typed-size-*` receipts.
The parity, actual-work cancellation, capacity and owner probes are authored
but unexecuted pending the consolidated validation gate.
