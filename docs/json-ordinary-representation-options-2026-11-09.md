# Ordinary JSON representation options

This source audit follows the #809 private-sentinel decision and compares
approaches without activating runtime behavior. Source baseline `5ad2d757`;
no executable tests have been run. The already transformed contents of a
stored record cannot be inferred or restored merely by changing the decoder.

`types::Value` is a public `Debug + Clone + PartialEq + Serialize + Deserialize`
enum with `Json(serde_json::Value)`. Adding a raw/arena variant would break
downstream exhaustive matches and add a serialized enum shape. `BatchRow` is
private and carries column `DataType` and owner links, but `RowAccess::get`
returns `&Value`; `eval_column_value` converts `Value::String` to scalar text
without consulting the JSON column type. A raw String substituted into an
existing `TYPE_JSON` serde encoder is serialized as a JSON string, not an
object. Existing JSON/JSONB output, NULL, field alias and expression behavior
therefore cannot be preserved by changing one value at that boundary.

Cassie's `Midge::put_document` takes `serde_json::Value` and row-blob
`encode_row` serializes each JSON field as bytes; `decode_row` and projected
variants deserialize to serde Value. Their callers include streaming and
ordered document scans, document mutation/read, column-store compact fields,
full-text, graph/time-series, vector publication, indexes and query source
hydration. These are Cassie adapters and consumers. Published cntryl-midge
0.5.2 exposes byte-key/value transactions; the storage engine need not change
and no second storage layer is selected.

Option A adds an internal schema-typed, owner-coupled raw/arena JSON cell and
parallel row decode/encode path, retaining the current public `Value` enum and
serde representation. The existing TYPE_JSON bytes can carry ordinary JSON
text without a persisted tag change, subject to proof of old/new readers and
exact canonical/diagnostic behavior. SQL hydration, expression evaluation,
semantic keys, Bind/COPY, joins, projections, arrays and every listed adapter
must migrate coherently before the new path is enabled. Public APIs returning
serde Value need either a separately proven conversion or an explicitly
retained legacy boundary; there is no blanket claim that a raw internal cell
can satisfy those APIs. Versioning, memory ownership through clones and
`into_entries`/`into_values`, cancellation, row-blob format compatibility and
already stored data require dedicated probes.

The result boundary is also public: `QueryResult::rows()` returns
`&[Vec<Value>]`, and its `Serialize` implementation serializes those Values.
An internal raw cell cannot replace a returned `Value::Json` object with a
String without changing the embedded API and serialized shape. A compatible
embedded result must materialize the expected Value representation or use a
separately proven equivalent. The infallible `rows()` accessor cannot report
an admission failure from a newly introduced lazy conversion. To avoid that
conversion for protocol clients, a private raw-aware result/encoder path would
need to carry the row owner through pgwire or REST independently; its control,
error, output and transport lifetimes require proof. The existing public
result remains unchanged until such a path is established.
Current pgwire simple-query handlers and connection writers iterate
`result.rows()`; extended portal state and COPY paths do likewise. A private
raw-aware path must therefore reach those consumers rather than only changing
the result builder.
Jev selected investigating a private schema-typed result and pgwire encoder,
with fallible materialization before constructing the unchanged public
`QueryResult` (A_design 1.00). Lazy conversion in the infallible accessor and
public API breaks were rejected. The public materialization/serialization
(0.63), pgwire/portal/REST (0.66), clone/owner lifetime (0.81) and storage/
old-reader compatibility (0.84) leads require probes before any activation.
Receipt: `target/assembly-review/json-raw-result-boundary-contract-{request,response}.json`.

### Prerequisites for a real row-to-pgwire path

A source-reviewed, test-only candidate carries synthetic existing-format
`TYPE_JSON` field bytes through an admitted directory, a plain-object cell,
and an admitted single-column DataRow. The candidate has no live callers and
its tests have not run. It proves no actual storage, SQL, socket or public API
handoff. PostgreSQL preserves JSON input spelling while Cassie's current
`Value::Json` writer compacts it; JSONB uses canonical text and binary version
byte 1. The selected wire behavior and its compatibility impact require
qualification, not an assumption that the two writers are byte-identical.

The first live path needs the following connected changes before activation:

1. **Preserve the input before it is lost.** Extended text Bind currently
   builds `Value::Json` via serde parsing, and JSONB first calls
   `jsonb::canonical`; `Midge::put_document` receives a serde Value and
   `encode_row` serializes that Value. A SQL write/read witness for a sentinel
   object requires a schema-typed owner through input, DML and row encoding,
   plus an unchanged TYPE_JSON byte layout. A synthetic row alone does not
   prove that fresh SQL writes preserve the object. Earlier transformed rows
   cannot be reconstructed.
2. **Carry the private cell through scans and execution.** Batched
   `scan_row_blobs_batched` and controlled `streaming_scans` currently call
   `decode_row` or a projected decoder and create public `DocumentRef` with
   `payload: serde_json::Value`; `AccountedDocument` owns that representation.
   `BatchRow` contains `(String, Value)` entries and its lookup and consuming
   handoffs expose those Values. A private sidecar or parallel internal row
   needs explicit per-field schema identity, SQL NULL, alias, query/operator
   owner transfer and fallible clone semantics through the actual chosen scan,
   projection and every supported operator. A fast path cannot silently skip
   authorization, filters, aliases, ordering or snapshot rules. Unsupported
   shapes must retain their established semantics; merely passing a raw JSON
   String as a `Value` does not satisfy this prerequisite.
3. **Carry the result to real consumers.** The existing public `QueryResult`
   and simple/extended pgwire paths consume `Vec<Value>` rows. The private
   result must survive worker and portal lifetimes and feed the real writer;
   public embedded and REST consumers must use a separately proven fallible
   materialization or remain on an explicitly compatible route. The isolated
   frame's lease ends at the frame owner and does not establish network-buffer
   or socket-flush ownership. Error mapping and resource/cancellation ordering
   must be preserved or explicitly selected and tested.

No one of these changes independently repairs #809. Qualification needs a
fresh write, scan, SQL projection and text/binary wire round trip for ordinary
and nested sentinel objects, SQL/JSON null, duplicate keys and malformed input,
as well as public-result compatibility, legacy stored rows, admission,
interruption and owner-drop probes. Midge remains the direct published byte
storage API; no new storage layer or upstream engine changes are implied.

Option B preserves all current signatures and builds serde Values directly
from the plain-object arena. This avoids sentinel-aware deserialization at the
initial conversion, but every later deserialization must also use the plain
policy. With pinned serde_json 1.0.154 and no `preserve_order` dependency,
`Map` is backed by `BTreeMap`, `with_capacity` does not reserve space, and
String-key comparisons and node allocations are private to the map. A
cross-toolchain, before-allocation admission bound and bounded interruption
for those operations has not been established. Public `Number::from_str`
likewise needs a full-length allocation/interruption proof; the hidden
`Number::from_string_unchecked` is documented as non-public API. Keeping an
arena alive alongside a newly allocated Value does not account for the new
Value. Accepting guessed limits or treating unadmitted map work as complete is
not selected.

Option C keeps both runtime routes unchanged while implementing focused,
unwired tests and owner-coupled primitives, until either A or B is evidenced.
Any production migration must preserve ordinary sentinel keys at all depths,
JSON versus JSONB input and output semantics, duplicate-last, numeric range,
NUL and SQLSTATE/error order, PostgreSQL wire OIDs, restart and cross-version
reads, memory/timeout controls and full scan/filter/projection/join behavior.
Use existing Midge APIs only. Jev selection and exact execution gates are
not executable proof. Jev selected investigating option A as a design and
test-only path (0.66; C 0.33), with no runtime activation selected. Its leads
are public API loss (0.81), storage and old-reader behavior (0.87), resource
accounting (0.83), and downstream reparsing (0.75). Receipt:
`target/assembly-review/json-typed-raw-cell-contract-{request,response}.json`.
No claim is made that Midge's existing Value-facing Cassie adapters carry the
new cell or that already transformed bytes are recoverable.
