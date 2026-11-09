# Scalar-window type metadata accounting

The scalar-window row reservation counts shared `DataType` vectors by the
underlying `Vec::as_ptr`, including the shared dangling pointer of empty vectors.
The old helper uses an unadmitted `HashSet` and recursive ARRAY traversal. Its
other callers keep their existing behavior.

The selected window-only replacement first counts present type vectors, admits
an exact reference directory, then collects and sorts it in place with checked
heap operations. Deduplication uses the original vector-buffer identity, not
the enclosing Arc. Each ARRAY chain is counted iteratively with interruption
checks. The directory is destroyed before its admission is released. Checked
arithmetic fails with ResourceLimit on overflow; representable totals retain
the existing per-vector, capacity and nested-type calculation. Allocator excess
over the requested directory capacity is rejected.

Jev selected this scoped contract (0.86; gap 0.93), receipt
`target/assembly-review/806-window-type-metadata-{request,response}.json`.
Focused probes must cover shared/distinct/empty buffers, capacity and nested
arrays; actual collection, heap sift and depth cancellation; exact/below
workspace admission, cleanup and the unchanged non-window helper. The newly
admitted workspace may make budget or cancellation errors occur earlier. Input
flattening, output ownership and allocator transient excess remain separate
limitations. Authored probes are unexecuted until the consolidated gate.
