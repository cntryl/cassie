# Serial accumulator result owner

For the normal serial accumulator path, the existing nonempty key reservation
travels with the result through output-row construction. A row without an
existing output reservation admits its existing directory and Arc wrapper
before absorbing that reservation; existing JSON-output reservations remain
in place. Query tracker identity is checked before absorption. Each output
clone continues through the existing clone admission path, and per-alias
interruption is checked after an actual append.

This deliberately retains some dead semantic-key charges until the last row
owner drops and can increase peak tracked usage. New row/directory admission
and interruption may precede later clone or alias errors. Allocator excess
may be observed only after allocation. This transfers the old reservation;
it does not prove it covers a distinct MIN/MAX result allocation, all nested
or Typed clone capacities, uninterruptible clone work, transition allocation,
or modifier/ordered-set/parallel accumulator paths. Those remain open.

Jev's choice is recorded at
`target/assembly-review/aggregate-accumulator-output-contract-response.json`.
Focused tests are authored before implementation and remain unexecuted until
the consolidated validation gate.
