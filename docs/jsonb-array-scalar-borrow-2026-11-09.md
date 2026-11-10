# Native JSONB array scalar borrow

After the selected native JSONB array comparison, binary and BETWEEN still
convert each evaluated `ScalarValue::Array` to a freshly cloned `Value::Array`
before type selection. In the existing controls-present, inferred JSONB[] and
native-array case for typed column/column and column/parameter expressions,
compare the evaluated arrays by reference after the same
type and parameter-context checks. An unselected input follows the prior path;
an inconclusive selected comparison preserves the same later fallback. A
control error still propagates. Add clone-observer, binary/BETWEEN,
parameter/null/mixed-type, cancellation and overlap probes before wiring.

Row-to-ScalarValue construction and all unselected conversions may still clone.
The JSON serialization, fallback, allocator and teardown limitations in
`jsonb-native-array-comparison-2026-11-09.md` remain. Probes are unexecuted.
