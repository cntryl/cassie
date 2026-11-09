# INSERT SELECT source owner handoff

The selected partial repair keeps each SELECT result row's query and operator
owners in an admitted sidecar until INSERT has exhausted or dropped its lazy
payload iterator. The sidecar remains live through width validation, identity
policy (including discarded and recollected columns), defaults, and each
synchronous write. Source rows and cells are destroyed before their owners on
errors. VALUES uses an empty sidecar.

Sidecar directory admission and per-row interruption checks can now precede
later width or identity errors. Retaining all SELECT source owners through the
entire iterator deliberately increases the tracked peak and may reject tighter
budgets. Exact allocator excess can occur transiently before it is rejected.
The sidecar does not account for cell directories, converted JSON, defaults,
identity-generated values, or Midge internals; it is not complete accounting.

The Jev decision is recorded in
`target/assembly-review/insert-select-owner-contract-response.json`. Probes are
authored before implementation and remain unexecuted pending bundle validation.
