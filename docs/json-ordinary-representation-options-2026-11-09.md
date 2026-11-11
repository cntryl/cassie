# #809 JSON carrier decision

Status: current-format direction selected and implemented on 2026-11-10. The
older option analysis below is decision history. Cassie is unreleased; no prior
Cassie Rust API or storage layout must be preserved, and no migration path is
required. Executed evidence is in the [#809 round-trip gate](json-sentinel-roundtrip-gate-2026-11-09.md).

## Selected direction

Keep `serde_json::Value` as the current public JSON carrier for this increment,
but do not use its sentinel-aware text deserializer for Cassie JSON documents.
Use Cassie's controlled JSON parser with an ordinary-object policy, then
materialize the parsed value without reparsing through serde's private map
sentinels. This retains existing public signatures as a practical implementation
choice; it is not a cross-version API compatibility promise.

Jev's source-based choice favored `serde_value_custom_parser` at probability
0.65/confidence 0.53, over a Cassie-owned public tree at 0.29, raw source at
0.01 and none at 0.05. The action is to source-prove a plain-object parse and
conversion path. Jev is a lead, not correctness evidence. The request and
response are retained under ignored
`target/jev/milestone-triage-20261110/revised-representation-{request,response}.json`.

## Source evidence

At `bab19a57b5ebf996bf526f4af81826de250b275f`, `Value::Json`,
`DocumentRef.payload` and `QueryResult::rows()` expose serde Values. CRB1
`TYPE_JSON` writes `serde_json::to_vec` and reads it with
`serde_json::from_slice`; text Bind and JSONB canonicalization also deserialize
through serde Value. The existing `runtime::accounted::json_arena` is an
admitted iterative forest with exact number tokens, duplicate-last object
insertion, owned output routines and query-control checks. Its production parse
policy currently recognizes serde's private Number and RawValue sentinels. A
separate unmerged prototype adds plain-object parsing tests only; it is not
production behavior.

The public storage boundary remains Cassie's Midge adapter over the published
`cntryl-midge` API. Do not edit Midge or add another storage layer. The new
current format must restart and recover; no earlier Cassie format or reader is
part of the gate.

## Contract and implementation boundary

- `$serde_json::private::Number` and `$serde_json::private::RawValue` are
  ordinary JSON keys at the root and every nested position, including escaped
  key spellings. Parsing them must not reinterpret their values as serde
  internals.
- Keep SQL NULL distinct from JSON document null. Keep JSON OID 114 and JSONB
  OID 3802/array OID 3807 distinct. JSONB keeps canonical key ordering,
  duplicate-last semantics and exact decimal values from the Type Contract.
- A current-format row write, full/projected row read, operator execution,
  result serialization and restart must preserve the supported document.
  Already transformed bytes from an earlier unreleased build are not migration
  inputs and are not required to be reconstructed.
- The implemented path covers text and binary JSON/JSONB Bind, text and binary
  COPY, REST, casts, arrays, operators, row persistence and decode, result
  serialization, restart, and database-image restore. The round-trip gate records
  the current route evidence and ordered local validation. Independent JSON
  feature limits remain in the Feature Support matrix.

## Qualification status

The coherent bundle gate passed on 2026-11-10: the locked build and full test
suite, strict all-target/all-feature pedantic Clippy, formatting, and global
test-policy validation. Focused tests cover root/nested/escaped sentinels,
duplicate keys, exact numbers, SQL NULL versus document null, malformed input,
text and binary wire formats, current-format restart, resource admission,
cancellation, and failure cleanup. See the round-trip gate for exact commands
and route evidence; this design record alone does not close #809.
