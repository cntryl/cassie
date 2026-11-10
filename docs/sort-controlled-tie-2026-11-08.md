# Controlled SORT tie text

Controlled SORT builds tie text with the admitted iterative serializer and
transfers its output lease into the row-key owner. JSON and SQL arrays are
traversed without materializing a nested JSON tree; text copies and directory
moves check interruption. The uncontrolled SORT path remains unchanged.

Tie-only dimension prefixes use `lower.wrapping_add(length).wrapping_sub(1)`,
matching the former release-build bytes for every i32 shape and replacing a
debug-only overflow panic with that same text. `SqlArray::new` and Cassie
`array_fill` allow lower=i32::MAX,length=1, whereas PostgreSQL 18.6's
standalone oracle rejects that bound; this change does not modify SQL array
validation or claim PostgreSQL parity. Zero-length SQL fill canonicalizes to
an empty array, while derived serde can construct zero/negative malformed
dimensions. Nested JSON traversal preserves the existing behavior for those
shapes, including ignoring dimensions after a reached zero.

The change can shift resource/cancellation precedence, uses additional
overlapping reservations, and may reject tighter budgets. Fixed typed-format
scratch, generic typed formatting and Numeric formatter's maximum-width
quadratic CPU remain unqualified. The later carried-stride increment in
`sort-tie-linear-stride-2026-11-08.md` removes repeated suffix scans.
Allocator transient excess,
earlier input cloning, comparisons and
teardown remain separate limitations. The bound choice is recorded at
`target/assembly-review/sort-array-bound-semantics-contract-response.json`.
Test-first differential/edge, actual-work cancellation, owner and budget
probes remain unexecuted pending consolidated validation.
