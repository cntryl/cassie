# Checked aggregate semantic-key estimates

Aggregate group estimates use an admitted iterative traversal that reproduces
the existing len-based semantic-key estimate, including nested Array and SQL
array dimensions and zero for separately owned JSONB/record and Typed keys.
The old top-level sum could panic in debug builds or wrap in release on
overflow; the checked path returns ResourceLimit instead.

Traversal stack admission and interruption can precede a later group
admission or SQL error and can tighten the query budget. Temporary allocator
excess is not proven admitted. This change does not control semantic-key
construction, Hash/Eq/sorted drain, value provenance, or accumulator mutation
growth. Test-first parity, frame-move, cancellation and exact-budget probes
remain unexecuted until the consolidated gate. The source-backed Jev decision
is recorded at `target/assembly-review/aggregate-semantic-estimate-response.json`.
