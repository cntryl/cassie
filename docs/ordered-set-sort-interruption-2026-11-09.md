# Ordered-set sort interruption boundaries

The shared #806 percentile, mode and hypothetical-set sort checks comparisons
and each outer permutation origin, but initializes both index buffers, writes
the inverse permutation, and executes each inner swap cycle without checks.
Those loops can run for the size of the input. All three production callers
hold their vectors locally and discard them on error; a partially permuted vector
does not become a published result. Percentile has a value-first sorted owner;
mode and hypothetical-set local error-drop ordering remains an open limitation.

Keep the admitted index buffers and stable merge/permutation algorithm, while
checking interruption at each initialization and inverse write and after each
actual inner-cycle swap. Preserve ordinary ordering and the earlier reservation
order. Cancellation can now precede later comparisons or final results; it may
leave a private partially permuted input that is discarded.
Focused probes cover real initialization, inversion and multi-swap cancellation,
stable duplicate order, measured-peak/below temporary budget and failure cleanup.

This increment does not qualify allocator transient excess, rich
`compare_values`/`compare_rows` work within an individual comparison, source
value creation, nonselected result ownership or total destructor latency.
Authored probes remain unexecuted until the consolidated validation gate.
