# Controlled NUMERIC arithmetic: proposed first slice

Status: isolated design and test-first probes; no runtime behavior is changed.
Issue #813 requires rounding, truncation, floor, ceiling, modulo and power, as
well as resource admission, cancellation and result ownership. Existing tests
and the #812 prerequisite do not qualify those combined obligations.

## Current source boundary

`filter/function_call.rs` evaluates arguments into owned `Value`s and may grow
an `EvaluationMemory` using an input-size multiplier. The math dispatcher in
`filter/functions.rs` calls `math_functions::evaluate` without either execution
controls or a result owner. Its `to_numeric` clones the input, and
`numeric_math` calls uninterruptible `BigInt` arithmetic, including scale
alignment with powers of ten. A short input can request a much larger result,
so an input-size multiplier alone is not a proof of workspace admission.

The projection row-owner selection in `projection/character_casts.rs` is
currently driven by character-array and JSON expression analysis. It does not
select ordinary NUMERIC math functions. A returned `Value` by itself does not
carry a query-memory reservation. Controlled math therefore needs both a
result-owner handoff and a proven route from every selected caller to the
correct lifetime; passing controls to the arithmetic kernel alone is not
sufficient. Embedded paths with no controls or owner require separate scope.

## Candidate: controlled two-argument NUMERIC truncation

The first candidate is `trunc(numeric, integer)` when a checked execution
context and a same-tracker result owner are available. Preserve existing NULL,
argument evaluation, overload, scale clamping, exact-decimal and display-scale
semantics. The unselected and uncontrolled paths retain their existing behavior
until separately addressed. Selection must be based on the resolved builtin,
not a user-spoofable function spelling that bypasses routine precedence.

Proposed kernel: read the finite input's borrowed base-2^32 limbs into a
pre-admitted workspace, checking cancellation during actual copy. For widening,
multiply by bounded decimal powers using checked per-limb passes; for reduction,
divide by bounded decimal powers using checked per-limb passes, and restore
negative requested scale using checked multiplication. Retain the existing
zero/special and provably-zero shortcuts, with identical output scale and sign.
The implementation must prove limb-count bounds before allocation, ensure
fallible growth cannot publish a partial result, and drop allocations before
their leases on error. It must not add a size or iteration cap.

`num-bigint` 0.5.1's public `BigInt::from_slice` builds a fresh private limb
allocation, copying the input in one operation; its allocator capacity and
transient allocation behavior are not observable through that API. A separate
result reservation can be held over construction and transferred together with
the result, but it does not establish a fully interruptible final copy or a
proof of allocator-excess bounds. These are explicit remaining boundaries, not
claims of complete control.

Before runtime routing, establish which projection, filter, aggregate, ORDER BY,
COPY, nested-expression and embedded callers retain an owner. For selected
projection expressions, extend owner detection and hold the resulting
reservation until the output row releases its value. On every path, check
tracker identity before transfer, keep a pending value before its reservation in
drop order, and preserve existing error and cancellation precedence. Do not
route a context without a proven owner into an allegedly controlled path.

The required test-first probes include positive/negative, zero and nonfinite
values; widening and reductions across decimal chunk boundaries; negative and
clamped scales; just-below/exact workspace and result budgets; synchronized
cancellation in real copy, division and multiplication passes; owner mismatch;
drop and failed-transfer lifetime; NULL and routine precedence; and row
retention after evaluation. Differential results must compare the existing
uncontrolled arithmetic and pinned PostgreSQL semantics. These probes remain
unexecuted until the consolidated validation gate.

This candidate does not implement or qualify NUMERIC round, floor, ceiling,
modulo, power, FLOAT8 overloads, binary operators, no-owner calls, or total
teardown/allocator behavior. The complete #813 issue remains open.

## Decision and next action

The focused Jev contract review selected an isolated checked-limb truncation
foundation (0.83) over a production partial route (0.13); arithmetic,
ownership and final-copy evidence remains required (gap 0.93). The response is
recorded in the local ignored `target/assembly-review/numeric-trunc-limb-contract-*`
artifacts. Its proposed test-only foundation is not an end-to-end acceptance
increment, so it is preserved without implementation while higher-priority
independent work proceeds.

The concrete Cassie-side obstacle is that `Value::Typed(Numeric)` owns the
`BigInt` itself while query admission is held separately, and the published
`num-bigint` API does not expose a fallible, cancellation-aware transfer of a
pre-admitted limb buffer into its internal representation. Projection must
also activate and transfer an owner for these expressions, and nonprojection
callers need their own source-proven lifetime or must retain the uncontrolled
fallback. A future implementation must solve or explicitly qualify those
boundaries and carry the reservation through the returned value's entire
consumer lifetime. This is a Cassie arithmetic and ownership problem, not a
Midge API limitation. No executable validation has occurred.
