# Owned row JSON diagnostic proposal

This test-only candidate follows the row-byte document mode. It
does not yet alter a production decoder. The candidate removes the test-only
legacy Value reparse from the row-to-cell error path: plain row-byte parsing
produces an admitted owned diagnostic with the intended legacy
`invalid JSON in row blob: ` prefix and structured SliceRead-compatible
category, line and column. Cancellation/resource errors remain their original
`CassieError`; a plain-valid sentinel object is never passed through the old
sentinel-aware Value parser.

The existing bounded diagnostic writer uses a fixed stack buffer and an
admitted output String; this proposal needs a row-specific prefix and owner
handoff without detaching that String. The `CassieError::Parse` display adds
`parse error: ` and its descriptor maps to SQLSTATE 42601, so eventual public
delivery needs an owner-compatible error representation and exact formatting.
No public error variant or production routing is selected here. Source-level
equivalence of the complete row-byte parser, cancellation/allocator behavior,
EOF/UTF-8 ordering and all sentinel-specific compatibility remain pending.
The fixed 512-byte stack handles fixed parse messages plus two `usize` decimal
positions and the row prefix; overflow is reported as a control error rather
than silently truncating. Probes cover actual post-copy cancellation and
retained diagnostic ownership, but remain unexecuted.
Jev selected this test-only continuation (1.00), not production activation;
its full-parity (0.75), fixed-stack (0.58), owner/cancel (0.47) and error API
(0.61) leads require focused probes and source review. Receipt:
`target/assembly-review/json-row-owned-diagnostic-contract-{request,response}.json`.
