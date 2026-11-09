# Compact JSON Value output

This partial increment serializes an already-owned `serde_json::Value` using
the compact representation of pinned serde_json with the enabled
`arbitrary_precision` feature. It preserves the existing map iteration order,
number spelling and escaping. Borrowed iterative frames and the output String
carry separate query reservations; replacement growth admits overlap and
checks frame moves, string scans and copies. The source Value and its owner
must remain alive until serialization returns. The returned String and its
reservation travel together; `into_parts` is only for callers that immediately
transfer both into an owner whose lifetime covers all later paths.

The selected scope and risk leads are recorded in
`target/assembly-review/json-compact-writer-{request,response}.json` in the
coordinator worktree. The parity, capacity, ownership and synchronized
cancellation probes are authored and unexecuted pending the consolidated gate.
This increment does not qualify input parsing, construction or destruction of
the borrowed Value, JSONB canonicalization, or output handoff and transport.
`try_reserve_exact` can transiently allocate capacity beyond the request; the
writer rejects and drops that allocation, without claiming pre-admission of
allocator excess. It introduces no new JSON depth or literal-size limit.
