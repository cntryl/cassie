# Raw JSON cell to private DataRow candidate

This test-only #809 experiment carries an actual TYPE_JSON row field through
the checked directory into an owned schema-typed cell, then builds an admitted
single-column PostgreSQL DataRow frame. It does not change QueryResult, public
Value/serialization, storage, portal, REST, socket or production routing.
Jev selected this scoped experiment (0.98); output/schema/format (0.90),
source/frame/null ownership (0.74), and scope/overclaim (0.71) require focused
unexecuted probes. Receipt:
`target/assembly-review/json-raw-datarow-contract-{request,response}.json`.

PostgreSQL 18.6 `json` text and `json_send` preserve the input spelling,
including whitespace, duplicate keys and number form. `jsonb` canonicalizes
to duplicate-last object order/numeric form, while `jsonb_send` prefixes that
canonical text with byte 1. The candidate emits the cell's raw JSON or
canonical JSONB bytes for both text and binary, adding that version byte only
for binary OID 3802. SQL NULL uses DataRow length -1; JSON null is the four
bytes `null` (plus JSONB binary version). It checks schema identity/OID,
format and representable lengths before allocation, admits the exact frame,
and retains the source owner throughout checked copies.

Current Cassie may compact a `Value::Json` for OID 114, so raw JSON bytes can
intentionally differ from its legacy writer for whitespace, duplicate keys,
key order and number spelling. Compare legacy only for representable ordinary
values and compare the raw witness with the PostgreSQL oracle; do not claim
unconditional byte parity or existing-row recoverability. Tests include all
four OID/format combinations, sentinel fields, SQL/JSON null, type/format and
length rejection, source/frame overlap, actual copy cancellation and release.
The synthetic TYPE_JSON test bytes prove a candidate wire path, not that a
particular historical database still contains the original object. All tests
are unexecuted, allocator transient excess and actual network writes remain
unqualified. The synthetic row and schema allocations themselves are test
fixtures, not query-budgeted allocations. The private frame helper collapses
schema, format and tracker mismatches to a prototype resource error and has not
established production diagnostic or cancellation precedence.
