# Window aggregate arguments with EXISTS

Status: source contract under review; no executable validation yet.

The focused Jev gate selected the separate controlled aggregate callback (0.88).
Its 0.87 lead requires source and focused probes for correlated/uncorrelated
resolver and search context, FILTER false/NULL suppression, actual inner-work
cancellation, allocation overlap, cleanup and non-aggregate siblings. The
receipt is retained in ignored `target/assembly-review/806-exists-window-control-*`.

Issue #806 requires window aggregate FILTER to be evaluated before arguments,
and requires resource admission and cancellation. `window_exec::exists` installs
an expression callback whenever this execution path resolves EXISTS. The
aggregate adapter currently uses that callback for every FILTER and argument;
`exists_phase::evaluate` evaluates expressions without a `filter::EvaluationMemory`
and, for ordinary expressions, without controls. This bypasses the aggregate
transition's argument owner even when the argument itself contains no EXISTS.

The proposed scoped repair gives the aggregate adapter a separate callback
accepting its existing `EvaluationMemory`. For an expression without EXISTS,
it calls the existing controlled projection evaluator with that owner. For an
expression containing EXISTS, it constructs the same phase outer row and
resolver as before, and calls the controlled resolving evaluator with the same
owner, parameters, search context, session and controls. The separate general
callback remains in place for non-aggregate window paths. The aggregate's
existing FILTER-before-argument order, resolver evaluation and short-circuit
rules do not change. This is not a claim that every allocation in the EXISTS
resolver or every non-aggregate window operation is fully controlled.

Test-first witnesses must cover an actual cancellation inside reached argument
work, false FILTER suppressing an invalid or cancelled argument, correlated and
uncorrelated EXISTS, returned value/temporary owner overlap and cleanup, and
argument/resolver errors. Probe budget denial and same-tracker owner lifetime
at the real transition boundary. Preserve non-aggregate window behavior and
document the inherited outer-row scratch estimate separately. All authored
Cassie tests remain unexecuted until the consolidated build/test gate.
