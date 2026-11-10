# Checked Value-backed JSON object keys

Extend the owner-carrying object-key function/projection path to an existing
`Value::Json` object without serializing or reparsing it. Keep the original
inspection scratch and the source Value alive while collecting borrowed keys
into two independently admitted directories and performing a checked stable
UTF-8 byte-lexical merge sort. The second directory drops after sorting; the
remaining sorted directory overlaps the admitted output Vec and key copies.
Explicit sorting matches the old `keys.sort()` even if a future
serde feature changes map iteration order. Adopt the completed Value-first
result and reservation into the same-tracker expression owner. NULL/non-object,
ownerless, general-owner and no-controls paths retain their existing handling;
the source Value's actual keys, including private-sentinel spellings, remain
ordinary keys in this route as in the old borrowed-Value path.

Jev selected the scope (0.95); ordering (0.85) and ownership (0.87) leads
require the focused empty/prefix/Unicode/NUL/sentinel, insertion-order,
non-object/NULL, real comparison/move/copy cancellation, same/foreign tracker,
and exact/below overlapping-budget probes. Receipt:
`target/assembly-review/json-object-keys-map-{request,response}.json`.
Additional sort/output admission and cancellation can precede the old result.
Earlier row-to-argument Value cloning, source physical capacity and Map iterator
internals, downstream expansion copies, allocator transient excess and teardown
remain unqualified. Tests are unexecuted until the consolidated gate.
