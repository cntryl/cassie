# Materialized projection payload owners

The selected partial repair retains each source row's query and operator
owners alongside the complete projection document batch until synchronous
`write_fresh_projection_output_rows` returns. Both batch directories are
admitted before allocation. Documents are destroyed before their owners and
the directory reservation, including conversion and publication errors.
Source owners are held conservatively for the entire batch, even after their
individual document has been consumed.

Width remains checked before conversion for each row, and collection drop,
creation and batch-write ordering remains unchanged. Directory admission and
per-field cancellation may precede a later width or conversion error, and
source plus directory reservations overlap. The allocator may transiently
return excess capacity before it is rejected.

This increment does not admit newly created field names, serde Map storage,
document IDs, deterministic hash serialization, value-conversion scratch,
encoded rows, or Midge internals. It does not claim full resource control.
The Jev decision is recorded at
`target/assembly-review/projection-payload-owner-contract-response.json`.
The focused test-first probes remain unexecuted pending bundle validation.
