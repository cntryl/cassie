# Percentile fraction ownership

The #806 percentile direct-argument path currently builds a scalar or array
fraction `Vec` before admitting its capacity, and walks array elements without
interruption checks. Its returned Vec and reservation are separate locals.

Jev selected a pre-admitted fraction owner (0.86, gap lead 0.67), receipt
`target/assembly-review/806-percentile-fractions-{request,response}.json`.
Admit an exact output directory against the same query tracker while the input
value and its reservation remain live, reject allocator capacity excess, and
parse each reached element in original order with interruption checks. A
field-ordered owner holds the output Vec before its reservation through
percentile evaluation. Drop the input value before its own reservation, and
never split the output owner into independently dropped locals. Reject a
foreign tracker before allocation. Preserve existing scalar, NULL, empty,
native/JSON array and error order when no control error occurs.

Output admission or cancellation may now precede an invalid later fraction;
allocation and overflow error messages can differ from the prior path. The
input expression, long NUMERIC/string conversion and diagnostic construction,
sorted values, result ownership, allocator transients and teardown remain
separate open limits. Focused overlap, cancellation, error-priority and cleanup
probes are authored but unexecuted until the consolidated validation gate.
