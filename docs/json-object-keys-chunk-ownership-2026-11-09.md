# Guarded JSON object-key chunk handoff

For the existing object-key expansion only, build the outer batch directory and
each batch's row directory under separately admitted capacity, retaining the
source row-vector Arc owner until its IntoIter is destroyed and checking each
actual row move. The completed batches precede their chunk-directory lease in
a private `OwnedExpanded` guard. Nonselected and exists-projection batches use
the same guard without a chunk lease.

The single production expansion caller in `source::apply_projection_phase`
keeps that guard while projection retention and budget checks borrow its
batches. DISTINCT takes the batches only while the guard remains live through
the consuming call; its own newly constructed directories are not covered by
the old chunk lease, which is conservatively retained until finalization. The
single `finalize_plan_rows` call takes batches under the guard, uses the existing
flatten, then releases the guard only after the input directories are consumed.
Errors and interruption drop contained rows/directories before the associated
leases. Batch buffer metric increments remain one per emitted nonempty batch.

Jev selected this guarded handoff (0.60; owner/branch/metric lead 0.84).
Receipt `target/assembly-review/json-chunk-handoff-{request,response}.json`.
Test-first empty/boundary/multiple, actual directory/move/flatten cancellation,
source owner, both DISTINCT modes, unselected path, metric and exact/below
peak probes remain unexecuted. Extra capacity overlap and cancellation checks
may change budget/error priority. DISTINCT-created directories, flatten output,
input source physical capacity, Arc deallocation, allocator transient excess,
lookup and teardown remain unqualified; the guard's lifetime claim is limited
to the audited private caller and does not qualify other bare Vec consumers.

The selected flatten allocates a distinct, separately admitted destination and
moves each row under interruption checks. This guarantees that input inner
buffers cannot be reused as the returned destination; input directories die
before their chunk lease. The destination's temporary lease covers its build,
but the returned bare `Vec<BatchRow>` has no directory owner after the handoff.
The added destination overlap and move checks may change budget or cancellation
priority. Untracked paths retain the original flatten implementation.
