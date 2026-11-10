# Rollup payload owner handoff

Each serialized rollup row keeps its source query and operator owners alongside
the JSON payload, including while the payload is moved to the synchronous
`Midge::put_document` call. Remaining payloads and owners survive until their
own publication or destruction. The payload directory is admitted before its
allocation and drops before its reservation on every exit. Source leases are
retained conservatively; they are not combined with a guessed payload charge.

The source-backed decision and Jev review are recorded in
`target/assembly-review/rollup-owner-contract-response.json`. CREATE and
REFRESH retain the existing publication and cleanup flow. Directory admission
may now reject a request before conversion or later publication failures, and
per-field cancellation can precede a later conversion error. Source and
directory reservations overlap, increasing peak tracked budget. Allocator
excess can be transiently allocated before rejection.

This is a partial owner repair. New serde object/map storage and conversion
allocations for Vector, Array, Typed values and other JSON intermediates are
not admitted by this slice. Midge's internal allocations and persisted data are
outside this ownership claim. The probes are authored before implementation
and remain unexecuted pending the consolidated validation gate.
