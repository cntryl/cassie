# Raw JSON cell error handoff candidate

This test-only follow-up preserves actual `CassieError` values through the
controlled row directory and inline cell instead of collapsing cancellation,
resource and directory failures into bare categories. It does not enable a
production decoder or change a wire or storage contract.

Jev selected a legacy-diagnostic fallback only when plain parsing fails and
the old TYPE_JSON decoder also returns an error (0.99). If the old decoder
succeeds, its Value must never be substituted for the failed plain cell:
private-sentinel interpretation or the inherited depth policy can differ.
The candidate must retain an explicit plain-error outcome instead. Receipt:
`target/assembly-review/json-row-diagnostic-contract-{request,response}.json`.
Legacy fallback scratch and formatting are unadmitted and uninterruptible,
diagnostic ownership/message/SQLSTATE and competing error priority remain
pending. This path is a compatibility probe, not a production-ready solution.

A subsequent test-only increment replaces that fallback with the row-byte
parser and an admitted owned row diagnostic. Its complete SliceRead parity and
handoff into public errors remain unqualified; see
`json-owned-row-diagnostic-proposal-2026-11-09.md`.

Probes cover exact cancellation and resource error variants, malformed raw
rows whose old decoder also errors, old decoder success when plain parsing
fails, and owner cleanup. All tests are authored but unexecuted.
