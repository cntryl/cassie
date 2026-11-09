# Checked aggregate group-size counting

Serial row/group and partial group entry-slice estimates use the independently
reviewed checked serialized-size counter. It preserves the existing compact
serde byte count and zero-on-serialization-error behavior for supported
values, with the prior generic serializer retained for unsupported shapes.
No supported SQL shape is rejected by design. Group signatures retain their
existing estimator, and accumulator retained-size accounting is unchanged.

The counter admits its traversal/formatting scratch and checks interruption.
This can reject a tight query budget or report cancellation before a later
group admission, SQL error, or generic fallback would have happened. Numeric
formatting remains quadratic at its maximum supported width and allocator
excess is observable only after allocation. Existing group-value expression
ownership, semantic-key construction, update/merge growth before admission,
output capacity and Midge/transport are not qualified by this change.

The wiring choice is recorded in
`target/assembly-review/aggregate-group-size-wiring-contract-response.json`;
the counter itself was source-reviewed as commit 27b83571. Test-first
differential, group lifetime, cancellation and exact-budget probes are
unexecuted until the consolidated validation gate.
