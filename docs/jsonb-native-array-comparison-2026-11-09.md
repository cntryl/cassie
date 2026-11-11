# Controlled native JSONB array comparison candidate

For the existing typed ARRAY comparison, retain its type inference, parameter
context, SQL NULL and element/length/dimension/lower-bound ordering. When both
already-evaluated operands are native `Value::Array` values, both inferred
element types are JSONB and statement controls are present, borrow those arrays instead of cloning them. For each
visited pair of `Value::Json` elements, serialize left then right with the
existing compact writer and retain both text owners while the controlled
Value-mode scalar JSONB comparator runs. Preserve its semantic-failure fallback
to the legacy pair comparator. Other element pairs and non-native arrays keep
their existing comparisons. The binary expression and BETWEEN routes propagate
control errors; no-controls execution keeps the current implementation.

Jev selected this scope (0.97); source probes must cover array semantics and
fallback (0.71), error propagation through binary and BETWEEN (0.68), and
source/dual text/arena lifetime (0.56). Receipt:
`target/assembly-review/jsonb-native-array-comparison-contract-{request,response}.json`.
Author differential, first-unequal/NULL/shape, actual-work cancellation and
exact/below-overlap probes before implementation. Introducing checked traversal
and text owners can make budget and cancellation errors occur earlier than the
previous result. Input `ScalarValue` to `Value` conversion may already clone the
arrays; element Values and earlier expression construction are not admitted by
this change. Generic element comparison, non-native conversion, fallback
reparsing, allocator transients, Value/arena teardown and ordinary sentinel-key round-trip semantics remain
unqualified. Probes are deferred to the combined gate.
