# Controlled JSON foundation: proposal and remaining proof

This records selected internal parser design directions and their outstanding
proof obligations; the full parser is not implemented or qualified. The
checked serde reader and a value owner form a useful partial increment, but
do not make every parse, comparison, conversion, error, or output operation
interruptible or fully admitted. See the Jev receipts
`target/assembly-review/json-parser-foundation-{request,response}.json` and
`target/assembly-review/json-parser-complete-{request,response}.json` in the
coordinator worktree. No probes described here have been executed.

## Observable boundaries

Binary JSON input currently validates the entire wire byte slice as UTF-8 in
`pgwire/connection/codecs.rs::binary_to_value`, parses it with
`serde_json::from_str`, and returns `Value::Json`. The existing codec maps both
UTF-8 and JSON errors to its binary invalid-data diagnostic. Its output path
renders the resulting serde value, so duplicate fields and original token
spelling are not retained. A replacement must preserve these behaviors and the
priority of invalid UTF-8 over JSON parsing errors.
Pinned serde's Value deserializer also classifies special private Number and
RawValue object keys in its first-key path; a generic duplicate-last object
parser cannot assume those cases are ordinary fields. Its arbitrary-precision
number scanner normalizes an exponent's `E` to `e` and inserts an explicit
positive sign in certain spellings. Source and differential probes must capture
those behaviors rather than merely preserving the raw input token.
In particular, a first `$serde_json::private::Number` key routes through
`NumberFromString`, while a first `$serde_json::private::RawValue` key routes
through `BoxedFromString` and calls a fresh `serde_json::from_str` on its
contents. The latter can reset serde's recursion counter for the nested source.
Compatibility frames must account for decoded-source ownership, nested error
locations and this depth behavior; rejecting these spellings or treating them
as ordinary maps would change the existing Value contract. A sentinel in a
later object position also needs a separate differential witness.

JSONB canonicalization is a different consumer: `types/jsonb.rs::canonical`
parses the complete document before formatting, then orders surviving object
fields by key byte length and bytes, formats numbers via NUMERIC, and rejects a
decoded NUL with SQLSTATE 22P05. Invalid JSON uses its 22P02 diagnostic. JSON
deletion has its own path and numeric-error precedence; it cannot be treated as
an interchangeable canonicalizer. Existing serde recursion behavior, including
its depth boundary and error position, must be preserved for each consumer.

Pinned serde_json 1.0.154 reads a checked reader byte by byte, but validates a
complete decoded string with `str::from_utf8` after input consumption. Its
object maps perform infallible String comparisons, and recursive values may be
dropped recursively. The binary codec also performs its own full-slice UTF-8
check before parsing. Thus a checked reader bounds input-read progress, not all
subsequent parser CPU or destruction. The existing 192-times-serialized-byte
reservation is documented in `runtime/accounted/json.rs`; allocator excess,
map split and duplicate replacement peaks, overlapping scratch and retained
values have not been proved for this application.

## Candidate for full control

One candidate is a Cassie-owned iterative tokenizer and value representation
whose frames, strings, numbers and ordered-object directories each retain a
capacity-coupled query reservation. An incremental UTF-8 and escape decoder
would preserve partial code points across chunks. Object lookup and duplicate
replacement would use a fallible, chunk-checked key comparator. Each growth
must admit old/new overlap, check potentially long moves, and keep the old and
new allocations attached to their owners on cancellation and errors. The
representation must support iterative destruction so a dropped document cannot
perform an unchecked recursive walk. This is a candidate to assess, not a proof
that the current consumers can accept it.

The consumer boundary is essential. Converting the result to
`serde_json::Value` still invokes infallible BTree comparisons and exposes
recursive destruction; it therefore cannot establish full interruption control
merely by following an iterative tokenizer. A complete design must carry the
owned representation through each relevant consumer and output serializer, or
provide and explicitly qualify a separately controlled handoff. It must not
replace JSON semantics with JSONB formatting or assume deletion's walker is a
general canonical parser.

Before implementation, independently establish how every malformed token,
line/column, duplicate field, number spelling and depth failure maps to the
existing observable result, and how parsed values, scratch, diagnostics and
network/output owners overlap. Record any public representation or compatibility
change before using it. Author differential tests for escaped UTF-8 and
surrogates, duplicate keys, nested arrays/objects, arbitrary precision,
recursion boundaries, invalid UTF-8 and malformed-before-format precedence;
add exact/one-below budget, duplicate replacement, mid-token, mid-comparison,
mid-copy and teardown cancellation probes synchronized at actual work. No
smaller literal-size limit is part of this proposal.

## Arena contract to resolve before implementation

The candidate representation is a private, in-memory arena, not a persisted
format or another storage layer. Each frame identifies its source buffer,
cursor, serde-compatible recursion state, syntactic context and destination.
Each source, token, node, key, child directory and output buffer must have an
owner that outlives every borrowed view. A replacement allocation must be
admitted while its predecessor remains charged; cancellation during a move
must drop each live allocation before its lease. Releases, including partially
constructed documents and nested decoded sources, need an iterative ownership
plan that does not depend on walking an arbitrarily deep Rust value tree.

Jev selected an internal owner-coupled arena and structured owned diagnostics
(0.97); see `target/assembly-review/json-arena-api-{request,response}.json` in
the coordinator worktree. The intended internal entry points are a UTF-8
source parser and a binary-input wrapper that checks all bytes before syntax.
The result owns its nodes and nested decoded sources, retaining any borrowed
outer input's existing owner for the full arena lifetime. Each allocation
precedes its reservation in its structural owner. The parser distinguishes
interruption and resource errors from invalid input. Invalid input carries an
owned diagnostic with serde-compatible category, line, column and any
custom/nested message, retaining the message reservation through every caller
handoff. Consumers map that diagnostic to their existing SQLSTATE and message;
binary COPY's invalid-data mapping and JSONB's TypeInput mapping remain
distinct. The exact Rust interface may remain crate-private, but it must make
those ownership and error branches explicit.

This design does not rely on fabricating `serde_json::Error`: its syntax/EOF
codes and position-fixing constructors are private in pinned serde_json
1.0.154. `serde::de::Error::custom` can carry a message and position but
classifies it as Data, so it cannot reproduce arbitrary syntax or EOF category.
The custom diagnostic's text, categories, nested-error wrapping, caller
conversion and error-memory handoffs require source and differential proof;
the selection is not evidence of that parity. Existing general resource-error
ownership is likewise not claimed complete.

The tokenizer must reproduce the existing consumer's operation order. Binary
COPY must check all input bytes as UTF-8 before attempting JSON syntax, because
that is the existing codec order; incremental UTF-8 validation must carry an
unfinished code point between chunks. Within JSON syntax, count lines and
columns, validate escapes and surrogate pairs, enforce serde's existing ordinary
nesting limit and track its error position. A RawValue sentinel creates a new
decoded source and a fresh serde parse context: retain its input and lease until
that nested parse returns, propagate its error through the same wrapping route,
and resume or reject the enclosing object as serde does. The nested parse runs
before the enclosing `end_map`, so a malformed decoded RawValue must win over a
following outer member or trailing-comma error. Do not introduce a
global substitute depth cap that rejects currently accepted sentinel nesting.

For ordinary objects, classify only the first decoded key against the two
enabled serde private tokens. `NumberFromString` requires a JSON string whose
contents are parsed as a Number; `BoxedFromString` requires a JSON string whose
contents are parsed afresh as a Value. Those visitor branches return before
reading another member, and serde's `end_map` then rejects a following comma.
For a non-sentinel first key, later token-shaped keys are ordinary fields.
Ordinary duplicate fields keep the last value. Retained numbers must follow
serde's `scan_integer` and `scan_exponent`, including its small-integer path,
`-0`, conversion of `E` to `e`, insertion of `+` where absent, and arbitrary
precision; a borrowed raw token is not always the retained representation.

Jev selected an incremental checked balanced tree as the object-storage design
direction (0.73); see `target/assembly-review/json-arena-object-{request,response}.json`
in the coordinator worktree. Retain-and-sort would keep duplicate values longer
and can alter peak budget and the order of resource versus syntax failures;
a digest index adds an unproved hashing and collision-security contract. The
selection is not a proof of an implementation.

The candidate tree uses stable arena indices rather than pointers into movable
buffers, fallible bytewise lexical key comparison in bounded chunks, and
checked, admitted node/path-directory growth. Keep the tree balanced and prove
its ordering, height and rotation invariants; do not add a field-count or depth
cap. Parse and own the entire new member value before key comparison or map
mutation: serde's `next_entry` completes that parse before `Map::insert` and
can therefore report its syntax error first. A duplicate keeps the already
stored key, discards the candidate key and replaces the previous value only
after the new one exists; both values and their leases coexist during that
handoff. An interrupted insertion must either leave a valid reusable tree or
abort and safely destroy the entire unpublished object without unleased data.
Plan and admit iterative disposal of displaced values and partial nodes; do not
assume recursive destruction or an in-place Vec reallocation is interruptible.
Iteration must match serde's lexical String order for compact JSON; JSONB's
later length/byte sort remains a distinct consumer operation. Exact budget,
allocator-excess, sentinel, error and teardown behavior remains to be proven.

The arena must not be materialized into an ordinary `serde_json::Value` and
then described as fully controlled: serde's Value map insertion and recursive
drop are independent unchecked boundaries. Each caller needs an explicit
handoff: compact binary JSON output must use serde Value's number, escaping and
map order; JSONB must finish parsing first, use length/byte key order, reject
surviving decoded NULs and retain NUMERIC diagnostics; deletion must preserve
its selected-path/error precedence. For every handoff, the resulting value or
text and its reservation must remain together through error, clone, return and
transport ownership boundaries. Record external/public compatibility changes
before selecting them.

The differential probe matrix must include both token names in first and later
positions, valid and malformed sentinel payloads, a sentinel followed by a
second member, and recursively encoded sentinel strings that cross the ordinary
serde depth boundary. Check exact error class, message and line/column against
the current consumer, not merely success. Also cover invalid UTF-8 preceding a
syntax error, partial multibyte boundaries, lone and paired surrogates,
malformed/trailing commas, duplicate keys and escaped-equivalent keys,
arbitrary-precision and exponent spellings, JSONB NUL/numeric precedence, and
ordinary deep arrays and objects. Resource probes must synchronize after real
UTF-8/escape scan, source or node allocation, duplicate replacement, key
comparison, output copy and partial-frame teardown; verify exact/below limits
and reservation lifetime through successful and failed consumer handoffs. All
such probes remain prospective until authored and executed at the consolidated
gate.

Minimum sentinel witnesses, with expected behavior to be checked against the
pinned serde oracle rather than inferred from this table alone:

| Input | Source-backed branch to compare |
| --- | --- |
| `{"$serde_json::private::Number":"12E2"}` | First key invokes NumberFromString and its number scanner. |
| `{"$serde_json::private::Number":12}` | First-key value must satisfy the NumberFromString string visitor. |
| `{"$serde_json::private::Number":"12","x":0}` | Visitor returns after the sentinel value; enclosing `end_map` encounters a comma. |
| `{"x":0,"$serde_json::private::Number":"12"}` | The sentinel appears after an ordinary first key and is an ordinary map key. |
| `{"$serde_json::private::RawValue":"[1,2]"}` | First key parses the decoded string as a fresh JSON source. |
| `{"$serde_json::private::RawValue":"[1,"}` | Nested parse error is wrapped at the original consumer boundary. |
| Nested RawValue sentinel strings around ordinary arrays | Each fresh parse has its own recursion state and decoded-source owner. |

Also vary escaped spellings of the sentinel key and value; classification is
on the decoded key rather than raw token bytes. Distinguish an explicitly empty
sentinel string, non-string sentinel value, trailing input inside the decoded
source, and trailing members outside it. Exact diagnostics and source positions
are still to be established by the deferred oracle probes.

## Compact Value output increment

Jev selected an independent compact `serde_json::Value` writer for source
implementation; see `target/assembly-review/json-compact-writer-{request,response}.json`
in the coordinator worktree. It borrows an already-owned Value and emits exactly
the pinned serde compact JSON representation: map iteration order, escaping,
and number spelling under the enabled `arbitrary_precision` feature. Its
iterative work frames and returned String have separate reservations. Growth
admits old/new overlap, keeps each allocation paired with its lease on failure,
and checks bounded scans and moves. The source Value and its separate owner
remain alive throughout serialization; the returned String and its reservation
travel together. There is no new document depth or literal-size cap.

This increment does not qualify input parsing, construction or destruction of
the borrowed Value, JSONB canonical formatting, or transport and network
buffers. Its consumer handoff must be wired and reviewed separately before a
COPY path is claimed controlled. Focused parity, capacity, lifetime and
synchronized cancellation probes are authored first and remain unexecuted
until the consolidated validation gate. `try_reserve_exact` may transiently
return allocator capacity larger than requested; the candidate rejects that
allocation and drops it, but does not prove pre-admission of that allocator
excess. The `into_parts` compatibility handoff is valid only if the caller
keeps the String and lease coupled through every later success and error path.

## Isolated object-index core increment

Jev selected per-node/payload reservations and stable slots (0.90), with
teardown (0.91), mutation atomicity (0.83) and duplicate retention (0.94)
requiring proof; receipt `target/assembly-review/json-arena-owner-{request,response}.json`.
The first isolated core implements an AVL object index with capacity-owned keys
and opaque stable value IDs. The parser must finish and own a new member before
calling insertion; duplicate insertion returns the displaced value ID so the
arena consumer can retire its subtree. The complete path and needed growth are
admitted and searched before tree mutation. Tree commit consists only of a
bounded number of pointer/index updates: a valid AVL tree containing at most
`usize::MAX` slots has height bounded by the machine word width times two; no
user field-count or input-depth cap is introduced. Comparisons and allocation
moves check interruption. If cancellation interrupts entry relocation, the
unpublished object becomes poisoned and must be discarded; it cannot be reused.

Object-index destruction is synchronous and flat, dropping each owned key before
its reservation; it is not claimed to provide cancellable teardown. Subtree
retirement, slot reuse across the arena, allocator overcapacity before its
post-allocation check, a real tokenizer and all consumer handoffs remain pending.
This standalone core has no production caller and is not a parser-completion
claim. All probes are authored first and remain unexecuted.

Jev selected generation-checked internal `NodeId { index, generation }` handles
for reusable value slots (0.93). Reuse increments the generation; a slot at
`u64::MAX` is permanently retired instead of wrapping, so a stale handle cannot
refer to a later value. The isolated slot increment uses an intrusive free list
and returns removed values together with their existing reservations. Slot
growth admits old/new overlap; interruption during movement poisons the
unpublished store and requires it to be dropped. Handle/slot reuse is only a
storage primitive: reachability, duplicate replacement, atomic publication,
subtree retirement and fully cancellable teardown remain pending. Receipt:
`target/assembly-review/json-arena-handles-{request,response}.json`.
The isolated value-slot prototype additionally rejects a value whose existing
reservation belongs to another query tracker before mutating the free list.
Its generic slot payload is a test boundary, not proof that future parser node
payloads are correctly sized; each real node must establish its own reservation.

Jev selected a checked, never-reused process namespace in each NodeId in
addition to slot generation (0.92); see
`target/assembly-review/json-arena-identity-{request,response}.json`. This
prevents cross-arena handle ABA even when stores share one query tracker; a
future logical arena uses one namespace for its shared store. Namespace counter
exhaustion fails closed instead of wrapping. Same-tracker cross-arena, dropped
owner, concurrent allocation, namespace/generation exhaustion and ownership
probes are required; they are unexecuted. This does not establish subtree
reachability, publication or teardown.

### Planned edge ownership and teardown

Jev selected a single logical arena with generation-checked node identities,
explicit unattached, root and parent ownership states, and separately owned
object directories (0.97). Before an edge is published, the arena must verify
that the candidate is an unattached subtree root, walk the target's parent
chain with interruption checks to reject self and ancestor cycles, and admit
all needed planning and growth. Edge and parent-state publication must be one
infallible transition. Duplicate replacement retains both subtrees through
publication, then detaches the displaced subtree for retirement. An object
directory rejects a key from a different query tracker before mutation. Stale
or cross-arena identities, repeated attachment, planning cancellation and
admission failure must leave the published forest unchanged.

Jev selected explicit cancellable retirement planning followed by flat,
synchronous retirement (0.72), with all owners retained until their allocations
are freed. No generic deferred resource-owner queue was identified in the
current runtime; schema cleanup is a separate persistence operation. A flat
destructor avoids recursive tree destruction but its total time can still grow
with the number of live nodes: it is not a claim of prompt cancellation during
Drop. Cycle/ABA and cleanup/lease behavior require the focused probes above,
including cancellation at planning boundaries and complete cleanup on failure.
These are selected contracts, not implemented or qualified behavior. Receipt:
`target/assembly-review/arena-linkage-and-group-{request,response}.json`.

Jev selected recoverable two-directory chunk migration (0.97), rather than
moving the live store into a fallible replacement or sharing its chunk owners.
Each fixed-size chunk has one owner and its own admitted payload. An admitted
replacement directory is fully initialized with empty slots before migration
starts; its lease overlaps the original directory's lease. At frontier `f`,
every existing chunk below `f` belongs to the replacement and every existing
chunk at or above `f` belongs to the original. Lookup, mutation and retirement
route by that frontier. Each bounded move takes one chunk from the original,
installs it in the replacement and advances the frontier without an intervening
fallible action. Cancellation between moves leaves this state valid and later
operations may resume it. Stable node IDs and the free list refer to logical
slot indices, not directory addresses. Completion frees the now-empty original
directory before its reservation. Destruction during migration drops both
directories and every chunk exactly once, with allocations dropped before
their owning leases; total synchronous Drop time remains proportional to the
live allocation count. No fixed total node or chunk count is introduced.

Directory and chunk capacities, replacement initialization, checked routing,
repeated cancellation, stable ID reuse, tracker identity and retirement during
migration still require implementation and proof. In particular, an allocator
that supplies excess capacity before the post-allocation check is an existing
qualification boundary. Receipt:
`target/assembly-review/json-arena-chunk-{request,response}.json`.

The isolated generic directory prototype exercises this frontier with owned
values, checked replacement initialization, mutation, retirement and reuse on
either side. The synthetic interruption probes exercise resumption while the
controls remain live; a real cancelled request remains cancelled and the probe
for it checks owner preservation and cleanup, not a successful retry. Actual
chunk integration, tracker identity, generation/free-list semantics, parser
ownership and production teardown remain unimplemented; none of the probes
have run.

The next isolated step applies that directory to the generation-checked slot
prototype. Each fixed-size slot chunk owns its admitted buffer; a node's index
selects its chunk and position independently of either directory's address.
Chunk initialization happens before directory publication, and the existing
namespace, generation and intrusive free-list checks remain authoritative.
Directory migration interruption must retain all previously live slot owners
instead of poisoning the store. Chunk size is an allocation granularity, not a
limit on total nodes. Object linkage, parent ownership and subtree retirement
remain separate obligations.

The isolated object index will use the same recoverable directory for admitted
fixed-size entry chunks. Existing AVL indices select logical chunk and offset;
moving a chunk owner must not move an entry or invalidate its index. Key
comparison, path and allocation remain fallible before the bounded AVL commit.
Cancellation during chunk migration keeps the previously published tree and
all leases intact; a chunk published before a failed insertion stays owned and
is reused. This is still an opaque-ID index, not a linked arena or parser.

The planned forest holds object directories in generation-checked node slots.
Under one exclusive forest borrow, it checks the candidate's unattached state,
walks the target's parent chain, finds any displaced node using checked key
comparison and verifies that node's parent before insertion. The object edge
and prevalidated parent-state updates then run as one infallible, uninterrupted
commit. No external callback, allocation or cancellation check may occur
between those updates. Existing object tests retain opaque integer handles while
the forest instantiates the index with `NodeId`. No consumer may acquire an
independent mutable index or forge node ownership. This is a selected internal
contract, not proof that parsing, retirement or user-visible integration is
complete.

The planned subtree retirement first builds an admitted, checked flat list of
generation-checked nodes reachable from an unattached root, verifying each
child's parent while traversing the sealed forest. Cancellation or resource
failure during planning leaves every edge and node intact. After a final
interruption check, a non-fallible commit removes and drops the planned nodes
without further checks; each node's object entries contain only IDs and keys,
so cleanup is flat rather than recursive through node ownership. Existing
single-parent insertion invariants exclude repeated reachability. This has no
prompt-cancellation guarantee once synchronous retirement begins, including
the fixed-depth directory and key cleanup inside a node; total latency may
grow with the live subtree. Allocator behavior and actual consumer teardown
remain qualification boundaries.

The isolated array increment applies the same unattached-child and checked
parent-walk rules to ordered `NodeId` edges. Capacity growth copies only the
IDs into a separately admitted replacement while the old directory remains
owned, checking interruption before publication. Array append and the child's
parent-state update are an infallible uninterrupted commit. Retirement plans
array children as well as object values. This remains an internal prototype;
value payloads, tokenizer, diagnostics and consumer ownership are still absent.

The private scalar foundation stores null, booleans and UTF-8 strings; a
string's buffer precedes its admitted reservation and is copied with bounded
interruption checks before the node is published. This only accepts already
decoded Rust `str` values. It does not implement JSON token parsing, escape or
surrogate rules, numeric normalization, sentinel reparsing, consumer output or
diagnostics; those remain separate obligations.

Jev selected an on-demand cursor and separately owned structured diagnostics
(1.00), with receipt `target/assembly-review/json-tokenizer-contract-{request,response}.json`.
The initial isolated cursor borrows source bytes for its entire lifetime; its
future owner must retain both the underlying buffer and its admission. A binary
entry validates the entire input as UTF-8 in checked chunks before any syntax
work; a `str` entry starts from already-validated bytes. Offset, line and column
are byte-based, starting at line 1, column 0; peeking computes the position
after the next byte without consuming it. Whitespace recognition is restricted
to JSON space, tab, CR and LF. Cancellation is checked during validation and
cursor advancement. The initial cursor's invalid-UTF8 marker has no claim to
preserve other serde error classifications or consumer messages.

Future tokenization must be demand-driven: decoding a RawValue sentinel's
nested source can fail before the outer map is complete, and that source needs
its own owner and fresh nesting counter. Value-string and ignored-string modes
have distinct control-character positions. Pinned serde advances across all
four hex bytes before validating a complete escape, moves to EOF on a short
escape, and distinguishes lone surrogates from missing or invalid second
escapes. Structured diagnostics and their owner, token grammar, sentinel
ordering, number normalization and consumer handoff remain unimplemented; the
initial cursor and its probes do not establish parser parity.

The next isolated token reads one Value-mode quoted string only when requested,
returning decoded text together with its reservation or a fixed-size structured
kind and consumed byte position. Buffer growth admits both old and new String
allocations while copying in bounded UTF-8-safe chunks. Resource or interruption
errors may occur before a later syntax error, as already documented for
controlled parsing. Value-mode control, short/invalid hex and surrogate
positions follow pinned serde's consumed-byte rules; ignored-string mode, raw
sentinels, nested errors, exact downstream diagnostics and public consumer
handoffs remain unimplemented.

Jev selected separate on-demand Value-prefix and strict NumberFromString
number modes (0.99), receipt `target/assembly-review/json-number-token-contract-{request,response}.json`.
Value mode scans an optional minus, integer, optional fraction and optional
exponent, leaving later delimiter/structural checks to its caller. The pinned
arbitrary-precision scanner emits lowercase `e`, retains an explicit exponent
sign or inserts `+`, preserves exponent digits and `-0`, and can represent
arbitrarily long exponent and integer text. The normal and strict modes require
separate error handling: strict mode skips no whitespace and any remaining
byte overrides a prior number-syntax error with InvalidNumber at peek position.
Actual query interruption is not replaced by that syntax error. The token
owner retains its admitted text buffer, including during old/new growth.
Value-to-Number fast-path representation, nested sentinel diagnostic wrapping,
full grammar, ownership handoffs and consumers still require proof.

The isolated literal token consumes `true`, `false` and `null` on demand,
leaving subsequent bytes for the container or document parser. Its structured
error distinguishes end-of-value, an unexpected value starter at peek, and a
mismatched consumed identifier byte, following pinned serde positions. Token
recognition alone does not implement document framing or nested sentinel modes.

Jev selected zero-copy adoption of a decoded token's String and reservation
(0.90), after checking the token and forest use the same memory tracker;
receipt `target/assembly-review/arena-token-handoff-{request,response}.json`.
Object keys use the same transfer, but a complete candidate value must already
be owned before the key is compared. String and normalized-number node tags
remain distinct. If later node or edge admission fails, the adopted payload
must drop before its lease; duplicate keys retain the prior key while releasing
the new key owner. These handoffs are internal and do not establish tokenizer,
sentinel or consumer integration.
Under pinned serde's arbitrary-precision mode, Number identity follows its
stored spelling: `1`, `1.0` and `1e+0` must not be collapsed by this handoff.
JSONB numeric equality and checked numeric/floating conversions have separate
consumer contracts and are not implied by retaining this text.

Jev selected borrowed-root/owned-nested source frames with ephemeral cursors
and saved positions (0.93), receipt `target/assembly-review/json-parser-source-contract-{request,response}.json`.
The future iterative document parser must not retain a cursor while changing
its frame directory: a frame step borrows its source, creates a temporary
cursor, performs bounded work and saves the resulting position before that
borrow ends. The binary root is UTF-8 validated before syntax; nested decoded
sources retain their original String and reservation, must share the parser's
tracker, and get their own fresh serde nesting counter. An admitted recoverable
directory owns active frames and reuses retired frame positions; partial moves
cannot lose earlier sources or reservations. Caller-owned borrowed input must
outlive the frame stack. Nested sentinel precedence, document/container error
mapping, failed-forest teardown and total synchronous Drop latency still need
proof. This frame contract does not yet implement the document parser.
Pinned serde attempts its container end check even after a visitor failure, but
returns the visitor error first. Matching the diagnostic alone therefore does
not prove identical cursor consumption or cancellation precedence during that
post-error check; those require separate parser probes and a concrete policy.

Jev selected an isolated ordinary-document grammar increment (1.00) using
admitted iterative array/object frames and fully parsed value ownership, with
the existing serde depth counter per source. The first decoded object key must
be classified before its value is processed: the NumberFromString and RawValue
sentinel branches are an explicit unfinished internal boundary in this
increment, not ordinary keys and not a production feature. They must both be
implemented with their exact nested/error ordering before any consumer wiring.
Container end checks after visitor errors, diagnostic category/position,
partial forest cleanup on cancellation or denial and source lifetime require
direct differential probes. The existing serde limit starts at 128 and rejects
the 128th ordinary nested opener; nested RawValue sources start a fresh counter.
No runtime registration or parser-completion claim is authorized here. Receipt:
`target/assembly-review/json-parser-sequencing-{request,response}.json`.

Jev selected the isolated quoted NumberFromString sentinel increment (1.00),
receipt `target/assembly-review/json-number-sentinel-contract-{request,response}.json`.
After the colon and a fully decoded string payload, its owner stays live while
the strict number scanner runs. A strict-number diagnostic becomes a structured
Data error with the inner position and existing message; it precedes any outer
map-end error. On success the parser requires immediate map end, then replaces
the unused empty object with the normalized number. The wrapper retirement can
require admission and therefore can introduce a resource error before a later
operation; this is a known partial boundary. Non-string sentinel payload type
diagnostics, post-error end-check cursor movement, RawValue, consumer error
ownership and production integration remain unimplemented.

Jev selected fixed-size structured unexpected-type diagnostics (0.97), receipt
`target/assembly-review/json-invalid-type-contract-{request,response}.json`.
For non-string sentinel payloads, serde consumes and validates null, boolean
and number tokens before a Data error, while sequence and map errors occur at
the current position without consuming their opener. Malformed scalar tokens
retain their earlier syntax/EOF diagnostic. A fitting unsigned or signed
integer is represented as its bounded integer value; `-0`, decimal/exponent
numbers and out-of-range integers use the generic number classification. The
NumberFromString expected phrase is distinct from RawValue's; actual message
rendering and its admission through consumers remain separate obligations.

Jev selected iterative RawWaiting frames with the owned nested-source stack
(0.98), receipt `target/assembly-review/json-raw-sentinel-contract-{request,response}.json`.
The decoded source is moved with its reservation, parsed using a fresh serde
depth counter and retained through nested diagnostic wrapping. A completed
nested result is kept while its source is popped and the outer sentinel's
immediate map end is checked. Nested syntax/data errors become a flattened
structured Data cause with the same nonzero inner position and one message
suffix; repeated nested wrapping does not add further suffixes. Control and
admission errors remain control errors. Pinned serde also performs outer
end-map checks after a visitor error even though the visitor error wins; this
cursor/cancellation behavior, exact consumer-owned diagnostic formatting and
runtime promotion remain explicitly pending.

For the binary-COPY consumer, Jev selected checked post-error end unwinding with
control errors taking precedence if observed during that scan (1.00), receipt
`target/assembly-review/json-binary-unwind-contract-{request,response}.json`.
This matches the current checked-reader adapter, which checks its interruption
slot after serde returns even when serde retained an earlier visitor error.
Without interruption, each entered array/object performs its end check once,
inside-out; crossing a RawWaiting frame wraps the inner diagnostic while its
source remains owned, then resumes the outer source. Sentinel map end checks
already attempted after a successful nested value are not repeated. JSONB and
other consumer policies, diagnostic formatting/admission and runtime promotion
remain separately unresolved.

Jev selected the existing admitted compact-JSON sink and escaping rules with a
separate checked arena traversal (0.93), receipt
`target/assembly-review/json-arena-output-contract-{request,response}.json`.
The immutable forest remains owned while its ordered array/object nodes are
visited; object keys follow the existing bytewise AVL order and number nodes
emit their retained serde spelling. A separately admitted iterative traversal
stack and the output String overlap the forest owner. The shared sink must
preserve its existing serde-Value writer and count callers. Parity for escapes,
Unicode, numeric/sentinel forms, duplicate keys and cancellation/budget owner
handoffs still requires source and executable proof; runtime wiring remains
unqualified.
The current number-node constructor accepts the same private decoded-text
carrier used by strings; only the sealed parser call path establishes that a
number was checked. A distinct validated-number owner or an equivalent
structural guarantee is required before a production consumer can use it.
The locked serde_json dependency has no indexmap/foldhash edge required by
`preserve_order`, supporting the current BTreeMap key-order assumption.

Jev selected an opaque scanner-produced `NumberToken` (1.00), receipt
`target/assembly-review/json-arena-number-token-contract-{request,response}.json`.
Its payload and constructor are private to the checked Value-prefix and strict
number scanner; the forest accepts only that token, checks its tracker while
intact, and then moves its existing buffer and lease into a distinct Number
node. The handoff adds no grammar, size or error-order change. Audit all token
constructors, malformed input, strict sentinels, cancellation and ownership
before production; this wrapper alone does not qualify a parser or consumer.

Jev selected an isolated arena JSONB formatter prototype (0.98), receipt
`target/assembly-review/json-arena-jsonb-format-contract-{request,response}.json`.
Formatting begins only after the complete document and duplicate replacement;
object keys are visited by UTF-8 byte length then bytes, with key-before-value
and depth-first error order. Surviving NUL in a key or string retains 22P05;
each number must match `numeric::parse_text` and its Display spelling and
TypeInputError, including format limits, exponent overflow, scale and signed
zero. Sorted-key workspace, traversal, output and diagnostics overlap the
still-owned source and must be admitted and interruption-aware. The prototype
does not qualify production JSONB, error transport, allocator transients or
synchronous teardown until those contracts are proved and executed.

# Binary COPY JSONB diagnostic handoff

Jev selected scalar OID 3802 wiring only after exact diagnostic and ownership
are established. The established order is version, complete UTF-8 validation,
complete document parse, then canonical formatting. Syntax diagnostics preserve
the existing serde message and byte-based line and column, including nested
sentinel wrapping. The COPY response keeps its existing SQLSTATE and prefix.
Malformed, version, UTF-8, NUL and numeric-overflow paths must not be collapsed.
Use structured diagnostics and bounded stack formatting followed by admitted
output with its owner retained through the wire write. Control errors propagate
separately. Probe every diagnostic variant, number and position boundaries,
nested wrapping, exact and denied admission, cancellation and response lifetime.
The fixed stack capacity is an internal bound proved from fixed messages and
integer widths, not a bound on accepted input. This design does not qualify the
arena parser, allocator transients, synchronous teardown, JSONB arrays or the
current Midge read path. Until source and executable evidence establish parity
and owner transfer, the existing COPY route remains selected.

The selected COPY handoff carries the private owned JSONB diagnostic through
the blocking worker in a private COPY error variant. It preserves the existing
`42601` state and full message, and retains the original diagnostic while the
existing admitted pgwire message, payload, and frame overlap it through the
asynchronous write. Existing worker accounting, explicit/implicit transaction
failure, ReadyForQuery, frame-limit closure, and non-JSONB callers must remain
unchanged. The controlled frame constructor is reused rather than introducing a
new public `CassieError` variant. Scalar JSONB alone is selected; array codecs,
parser parity, synchronous teardown, and all executable validation remain open.
If constructing the admitted response encounters a query limit or cancellation,
the existing generic pgwire mapper supplies the resource or cancellation error;
the normal COPY failure/rollback and ReadyForQuery path still runs, and the
original JSONB diagnostic remains owned until that response completes.

## JSONB array envelope and dual output

Jev selected an isolated JSONB array prototype retaining the existing array
decoder's six-dimension maximum and all error order. Validate header and the
minimum element-length area first; validate each non-null JSONB element in wire
order before checking trailer, null flag, document-null rejection and final
shape. SQL NULL and JSON document null are distinct. Keep the payload owner and
admit each retained element range and every rendering workspace. After validation,
reparse retained element slices for output: this makes a later query-memory or
control failure possible, but does not silently change accepted values or input
syntax. Normal output uses the existing SQL-array quoting and dimensions; when
the existing bounds formatter fails despite a valid shape, output the original
compact nested JSON representation instead of canonical JSONB. Preserve the
existing empty-array and exact element error diagnostics. The prototype does
not wire a live array route, qualify synchronous teardown or claim executed tests.
