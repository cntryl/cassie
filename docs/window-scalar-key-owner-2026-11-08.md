# Scalar WINDOW key ownership and stable sorting

The scalar WINDOW path admits peer/sort-part directories before allocation,
uses shared controlled SORT conversion for proven key shapes, and carries
each retained key and tie-text lease with its owner. The input
Value is dropped before its conversion scratch reservation is shrunk. The
SORT helper retains the existing special handling for native ARRAY(Vector),
finite NUMERIC, BIT and VARBIT and SEARCH precedence. Unproved shapes keep
WINDOW's original key conversion and post-construction length estimate; they
do not use SORT's stricter shape check. The eligibility scan can encounter
interruption before SEARCH extraction.

Scalar partition ordering uses an admitted keyed directory and a stable
two-buffer permutation. Equal comparisons retain input index order, and the
indices are changed only after successful sorting. Typed WINDOW keys keep
their existing collective owner. Frame and peer semantics are unchanged.
New directory/scratch admission and interruption can precede a later SQL
error, and overlapping reservations can tighten budgets.

This is partial: expression Value creation before conversion, generic and
nested key provenance/physical size, long infallible comparisons, the WINDOW
partition map, typed WINDOW accounting, allocator transient excess and
teardown remain unqualified. Fallback construction and recursive estimation
are uncontrolled and its length estimate does not prove physical capacity.
Numeric bigint's final slice copy and private
capacity also remain open. The decision is recorded at
`target/assembly-review/window-scalar-key-owner-contract-response.json`.
Test-first stable/parity, actual-key cancellation and exact/below probes are
unexecuted pending the consolidated validation gate.
