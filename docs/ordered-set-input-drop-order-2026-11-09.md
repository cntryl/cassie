# Mode and hypothetical-set input drop ordering

The #806 mode and hypothetical-set paths keep input vectors and their query
reservations in separate locals during construction. On an error those locals
can release reservations before the vectors they cover. This predates the
ordered-set sort interruption increment; its private partial vectors are not
published, but their drop ordering is still an ownership gap.

Use a private value-first `RetainedInputs<T>` guard for mode input,
hypothetical direct arguments and hypothetical ordered rows. Its vector drops
before both existing reservations on success and every error. Ordered-row key
payloads continue to use the direct-argument guard's retained reservation;
the ordered-row guard drops before that caller-owned reservation. Local key
buffers drop before their ordered-input guard if construction fails.

The hypothetical direct path creates its existing zero-byte retained lease
before the fallible Vec allocation rather than immediately after it. Under the
tracker invariant, reserving zero cannot overflow or exceed the budget; the
atomic operation can retry under contention. Existing value, sort and error
semantics otherwise remain selected. Each newly retained mode, direct or
ordered input is followed by an interruption check. The current value's parse,
clone and retention errors still occur before that check, but cancellation may
now preempt a later input's ordinary error. Drop sentinels, current-value error
and actual post-insertion cancellation probes are authored but unexecuted until
the consolidated gate.

This repair does not qualify mode's selected-result clone, hypothetical result
handoff, input conversion, growth relocation/allocator excess, rich comparison
work, general query cancellation or total synchronous destructor latency.
