# Checked object-key JSON preflight

For `json[b]_object_keys` with a `Value::Json` object, statement controls and
an expression owner, count the input's compact serde JSON byte length with the
already-reviewed checked compact traversal before the unchanged inspection
scratch reservation. The count stack drops before that scratch is admitted.
All other functions, input variants and ownerless/general/no-controls paths
retain the existing `serialized_json_bytes` counting writer. The selected
function then follows the existing checked map-key output handoff.

Source review selected this direct reuse: count bytes must match the old writer
for nested values, escapes, Unicode, arbitrary-precision numbers and sentinel
shapes. Test-first probes cover actual long-string scan cancellation,
count-stack release, exact/below total peak and unselected fallback. Added
pre-scratch stack admission and cancellation may take priority over later work.
Source/argument cloning, Map/Value private physical capacities, downstream
expansion, allocator transients and teardown remain unqualified. Tests and Cargo
checks remain unexecuted until the consolidated gate.
