# Plain-object arena candidate

This test-only foundation follows the #809 sentinel decision: JSON object keys
that happen to match serde's private Number or RawValue sentinels must remain
ordinary keys at every depth. PostgreSQL 18.6 oracle evidence and the wider
contract are recorded in separately reviewed `json-private-sentinel-semantics-2026-11-09.md`.
Jev selected a distinct ordinary-object policy with a cross-entry and storage
audit (0.98), receipt `target/assembly-review/jsonb-sentinel-cross-entry-contract-{request,response}.json`.

The candidate runs the existing owned, iterative arena parser with a private
test-only policy that suppresses sentinel dispatch; its default Value policy
is unchanged. No production input, operator or storage route is changed.
Fixtures cover both sentinel names at root and nested positions, escaped keys,
duplicate-last, later ordinary members, malformed syntax, owner release and
actual parser cancellation. Value-mode sentinel behavior remains covered by
existing tests. All authored tests are unexecuted.
The candidate inherits the current Value-mode nesting limit; no claim is made
that this limit matches PostgreSQL or that it is appropriate for a future
ordinary-JSON production route. That compatibility decision remains open.

Before promotion, audit JSON/JSONB cast and wire input, scalar and array COPY,
row-blob and Midge-adapter storage, semantic key/hash/order, joins, functions
and all serde Value conversion boundaries. `TYPE_JSON` currently writes Value
as JSON bytes and reads those bytes with `serde_json::from_slice`; therefore a
correct object can be reinterpreted at decode. Text Bind, binary JSONB codec,
`types::jsonb::canonical`, and `JsonbKey::from_text` are additional confirmed
Value-deserialization sites. Do not claim already-transformed data recoverable.
Preserve separate JSON/JSONB diagnostics and 22P05/22003 behavior, no new caps,
checked owners and the published Midge-only storage boundary. Production
adoption, teardown, diagnostics, error parity, resource behavior and the full
consolidated build/tests/clippy gates remain open.
