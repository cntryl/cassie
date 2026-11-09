# Percentile sorted state and null-array output ownership

The #806 percentile sort currently retains a Vec of ordered values and its
reservation as separate locals, so failure can drop the reservation first.
An array result is charged to that same temporary reservation but returned as a
bare Value. The private `AggregateOutput` and `OutputRow` support a same-query
result owner through row attachment; ordinary JSON cloning is still unchecked.

Jev selected a field-ordered sorted-value owner and a separate selected output
owner for array fractions containing only SQL NULLs, including the empty array
(0.67, gap 0.72), receipt
`target/assembly-review/806-percentile-sorted-output-{request,response}.json`.
Input validation, FILTER, value collection and sorting complete in the existing
order before this result is made; no sorted rows still produces SQL NULL. When
rows exist, pre-admit the exact null-only JSON array directory and retain it in
`AggregateOutput` through `OutputRow` and all duplicate-name copies. A private
null-array copy policy checks the source and copies each element with controls.
No serde JSON number or string formatting is involved in this selected result.

The empty array retains a zero-byte reservation and explicit private policy so
OutputRow creates an owner even though `AggregateOutput::from_owned` normally
discards zero-byte reservations. This may introduce an earlier resource error
for that result; output admission and checks may also precede later work. Each
value drops before the reservation covering it, including error and cancellation
paths; sorted, fraction, output and copy reservations overlap where needed.

Scalar results and non-null percentile arrays retain existing result paths;
in particular arbitrary `percentile_disc` clones, JSON numbers and strings,
nested outputs, input-expression clones, sort comparators, allocator transients
and teardown remain unqualified. Tests must cover scalar/array/no-row policy,
zero/exact/below budgets, actual creation/copy cancellation, duplicate aliases,
foreign trackers and output owner survival through final row drop. Probes are
authored but unexecuted until the consolidated validation gate.
