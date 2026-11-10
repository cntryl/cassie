# Subquery row ownership handoff

The selected partial repair admits the destination row directories and the
existing retained-value estimate before moving values out of `BatchRow`.
Both source query and operator owners remain live during each move. The
destination rows are destroyed before their reservation on success and error.
The result and its Arc allocation are included in the destination estimate.

Scalar/row cardinality remains checked before destination admission. Directory
admission and new interruption checks may now precede width errors, allocator
failures, and later row-limit errors. Source and destination reservations
overlap during transfer, so a query near its budget can now fail earlier. This
is intentional until exact provenance-aware transfer is available.

The existing recursive `value_clone_bytes` estimator uses SQL-array lengths
rather than capacities, and its Typed NUMERIC heap estimate is not proven
complete. After source owners retire, destination coverage for these values
is therefore unqualified. This slice does not qualify
their traversal, recursive destruction, or all source owners. Rust's allocator
may transiently return more capacity than requested before the excess is
detected and rejected; Arc allocation overhead is based on its two-word header
and remains subject to allocator rounding. These are separate boundaries, not
claims of strict allocator peak control.

Jev selected the partial owner/directory repair over claiming full ownership;
the receipt is `target/assembly-review/subquery-owner-contract-response.json`.
Focused probes are authored before implementation and remain unexecuted until
the consolidated build and test gate.
