# Aggregate success-output reservation handoff

The selected partial repair transfers existing serial or merged-group
reservations to the output rows, retaining them until the last row owner is
dropped. Partition and merge reservations must use the same query tracker;
this is checked before combining them. A shared Arc header and each new row
operator link are admitted before allocation. A pending-output wrapper keeps
all rows before the shared lease while attachment is incomplete, including
cancellation and admission failures. Existing row query and operator owners
are preserved.

The retained group estimate may include dead groups, input rows and other
intermediate charges. Keeping it for the last output deliberately tightens
budgets and can cause earlier failures. Header/link admission and interruption
can also precede later errors. Empty output needs no new row links. Allocator
excess and metadata remain subject to existing allocation boundaries.

This transfers the existing reservation only: serialized group estimates are
not proven to cover output capacity or cloned values, and accumulator
transition allocations, `AggregateAccumulator::evaluate`'s separate key
owner and returned value, generic serialization, and Midge remain unqualified.
Jev's choice is recorded at
`target/assembly-review/aggregate-success-owner-contract-response.json`.
Tests are authored before implementation and remain unexecuted until the
consolidated validation gate.
