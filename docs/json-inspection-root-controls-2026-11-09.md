# Checked JSON root inspection

For `json[b]_typeof` and `json[b]_array_length`, retain the existing arity,
SQL NULL and scratch admission order. When statement controls exist and the
input is a `Value::String`, parse with the existing owned Value-mode arena and
read the root kind or array length while that arena and the inspection scratch
remain live. If the arena reports a semantic parse failure, use the existing
`document()` path to preserve its diagnostics. Value-backed inputs, calls
without controls and `json[b]_object_keys` keep their current implementations.
Value-mode private sentinel behavior remains unchanged.

Jev selected this scope (1.00), with parity/sentinel/NULL (0.76), overlapping
scratch/arena ownership (0.64) and other-route/cancellation (0.67) leads.
Receipt `target/assembly-review/json-inspect-root-arena-contract-{request,response}.json`.
Added arena admission and checked parsing can change resource or cancellation
priority. Original input creation, scratch physical coverage, fallback parsing,
result String ownership, allocator transients, recursive teardown and the
ordinary-sentinel round-trip semantics remain unqualified. Focused differential,
actual parser-work cancellation and exact/below overlap probes remain
unexecuted until the consolidated gate.
