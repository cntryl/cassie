# Scalar discrete percentile primitive SQL Array ownership

The #806 scalar `percentile_disc` SQL Array result currently clones the sorted
value and returns it without a result reservation. The sorted input reservation
charges array lengths rather than potentially larger source Vec capacities, so
moving the input under that reservation alone is not a sound ownership proof.

Jev selected a separately pre-admitted checked copy for arrays containing
only NULL, boolean, i64 or f64 carriers (0.90; gap lead 0.77), receipt
`target/assembly-review/806-percentile-primitive-array-{request,response}.json`.
Retain existing fraction validation, NULL/FILTER/sort and exact rank order.
After selection, inspect flat elements under interruption checks; arrays with
any other carrier retain the legacy path. This adds cancellation priority on
that eligibility scan even for a later fallback. Admit destination dimensions
and element slots before allocation, reject allocator capacity excess, then
copy each dimension and element under interruption checks with a value-first
owner while the source is live. Preserve every f64 bit, all bounds, and the
exact source layout, including malformed values admitted by derived
`Deserialize`. A narrow crate-private copy method may construct the copy
internally; no general arbitrary-parts public constructor is added.

Transfer the copy and its same-tracker reservation, including zero-byte
reservations, through a private Array-only `OutputRow` policy. Validate the
private policy's element carriers and independently pre-admit and copy every
alias. Failure, cancellation and a foreign tracker cannot publish an incomplete
alias. Allocation or cancellation may now precede later alias work. Source,
result and row owners overlap until their respective values are destroyed.

PG18.6 produced `{1,NULL,3}` with `integer[]` result for two ordered integer
arrays, preserved `[0:1]={1,2}` and `[2:3]={3,4}` at fractions 0 and 1, and
returned `{t,NULL,f}` with `boolean[]` for a single boolean array. Regressions
must cover those forms and exact OIDs, invalid fraction/NULL/FILTER order,
empty and malformed deserialized shape, f64 bit patterns and i64 extremes,
actual dimension/element/alias cancellation, foreign trackers, measured
exact/below peak, partial alias and owner cleanup, and mixed/nested/String/
Typed fallback. Probes are authored but unexecuted.

This is the aggregate output and alias handoff only. `BatchRow::clone` shares
the origin lease without admitting its new deep allocations; `QueryResultBuilder`
later deep-clones SQL Arrays without interruption inside the clone. Input
conversion and source capacity estimates, comparisons, generic element types,
projection copies, group ownership, allocator transients and synchronous
teardown remain unqualified. This does not complete #806.
