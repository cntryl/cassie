# Row-byte JSON diagnostic mode

This test-only #809 candidate explores SliceRead-compatible error ordering for
JSON bytes already present in a row. It does not activate a row decoder, alter
binary COPY, or qualify the full parser. Jev selected the separate row-byte
mode (0.76); decoded-byte positions (0.66), escape/control/EOF precedence
(0.73), resource/drop (0.86), and source scope (0.80) need source and
unexecuted differential probes. Receipt:
`target/assembly-review/json-row-byte-diagnostic-contract-{request,response}.json`.

Pinned serde_json 1.0.154 `SliceRead::parse_str_bytes` scans through a string
and handles escape, control and EOF errors before validating the decoded UTF-8
at its closing quote. Thus an invalid raw byte earlier in a string can lose to
a later bad escape, control byte or missing quote. `as_str` computes the
invalid-byte position from the closing position and decoded byte length; with
escapes the column can be approximate. Current arena byte input instead
validates the whole source as UTF-8 first and reports no per-string position.
Binary COPY deliberately retains its existing UTF8-first entry contract.

The scoped candidate collects decoded string bytes in an admitted buffer,
checks escape syntax while scanning, and validates/copies bounded UTF-8 chunks
only after the closing quote. Both byte and String buffers and their leases
overlap until handoff; interruption and errors must drop buffers before their
reservations. Invalid-byte column arithmetic must follow serde's decoded-byte
accounting. Tests compare category and exact position/message for keys and
values, prior invalid bytes followed by escape/control/EOF, Unicode split
boundaries, escaped bytes, lines and actual cancellation. Any unproved source,
diagnostic delivery, row object traversal, allocator excess, legacy fallback
or transport behavior remains open; no size/depth limit is added.

The next test-only increment extends the policy to complete row documents:
the root source is read as bytes without whole-input UTF-8 prevalidation, and
each reached object key or string value uses the deferred string decoder.
Plain-object mode avoids Value-sentinel nested reparses. Existing Value and
binary COPY entrypoints still validate UTF-8 first. Unescaped invalid bytes
outside strings, all delimiters, trailing content, nesting and line/column
ordering must match the old SliceRead path for the tested non-sentinel corpus.
This remains an unconnected diagnostic probe; full error text ownership,
sentinel-related compatibility, row-cell wiring and execution remain pending.
