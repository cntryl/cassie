# Controlled NUMERIC literal validation

Issue #813's power coercion binds quoted unknown literals to the existing exact
NUMERIC carrier. The binding boundary must reject the same invalid input and
range cases as `numeric::parse_text` before committing to that carrier. This is
validation only: it does not allocate or return a `Numeric` value, change the
public numeric representation, or add a precision/depth cap.

The selected source contract is the current Cassie numeric input grammar:
ASCII whitespace at either end; plain case-insensitive `NaN`; signed or unsigned
`inf`/`infinity`; decimal integers/fractions with either side of the decimal
point optional but not both; decimal exponent and valid underscores; and signed
binary/octal/hex literals with one optional underscore following the radix prefix.
Separators elsewhere must be between valid digits of that radix. Preserve the
existing order of malformed-coefficient, malformed-exponent and range errors:
malformed input is `22P02`, while integer digits beyond 131,072 or display scale
beyond 16,383 and selected exponent overflows are `22003`. A zero coefficient
still observes the existing scale/exponent range rules.

The validator scans borrowed input in bounded chunks and checks cancellation;
no stripped copy of the literal is required. Decimal significant-digit/scale
counts use checked arithmetic. After validating the entire radix spelling, a
safe bit-length fast path uses `2^9 < 10^3 < 2^10`: bit length at most `3*N`
proves fewer than N+1 decimal digits; bit length greater than `ceil(10*N/3)`
proves overflow. The middle band streams radix bits into an admitted base-10^19
limb directory, multiplying by powers of two with checked u128 arithmetic and
checking interruption during conversion. Counting its highest limb determines
exact decimal digits. Partial binary words, leading zeros and all-zero input
must be handled exactly. The full spelling is validated before any range
answer, so a later invalid digit still wins over an earlier apparent overflow.

Jev selected the exact controlled validator (0.94), with semantic/admission
risk 0.84 and quadratic CPU risk 0.89. The middle-band conversion can do
quadratic work on very large radix inputs; no timing or performance claim is
made. Reserve each temporary buffer before allocation and release on all paths;
small private test limits exercise just-below/exact/just-above without replacing
the public limits. Authored grammar, radix, boundary, cancellation and resource
probes are unexecuted. `numeric::parse_text` remains the parity oracle.

Diagnostic ownership is a separate unresolved cross-phase issue: the public
`TypeInputError` contains an owned bare `String`, and the Cassie/pgwire error
descriptor clones it. Admission while building a syntax diagnostic alone does
not retain that reservation through error rendering. The diagnostic owner and
transfer boundary must be selected and recorded before claiming full error
lifetime accounting. No workflow configuration changes are authorized.

Jev selected extending the existing internal owned scalar-diagnostic route
(0.65; construction-only 0.21) while preserving the public `TypeInputError`
shape. Existing `ParameterAdmissionError::InvalidScalarLiteral` retains copied
input and its reservation, and `ExtendedQueryError::scalar_literal_error`
produces an admitted frame held through writing. That route currently covers
extended parameter analysis, not binder SimpleQuery or embedded errors.
Descriptor cloning and SimpleQuery/embedded error lifetime remain unimplemented
until each transfer and peak overlap is proved. Gap lead 0.91 requires
below/exact admission, cancellation during copy/frame generation, descriptor
cloning, network-write lifetime and embedded-error return probes.

## Selected diagnostic carrier and compatibility impact

Jev selected an additive owner-carrying `CassieError` variant plus fallible,
admitted descriptor/frame conversion on controlled pgwire Simple/Extended and
REST paths (1.00). The public `TypeInputError` fields and existing variant stay
unchanged. Adding a variant to public `CassieError` can break downstream Rust
exhaustive matches; this source-compatibility impact is intentional and must be
reported with the final bundle. The owned variant must drop its text and any
physical diagnostic data before its reservation and keep the owner on embedded
error returns. Descriptor/frame construction must admit overlapping old/new
buffers before copies and retain the frame lease through the actual write.

The existing infallible `descriptor()` and freely cloneable descriptor cannot
provide that ownership guarantee: an arbitrary caller can create independent
copies after the original lease drops. Keep that legacy API behavior for
compatibility, but explicitly treat those escapes as unbounded. Controlled
SimpleQuery, ExtendedQuery and REST paths must be audited and switched to the
new fallible conversion as applicable; no end-to-end accounting claim is valid
until every such transfer, clone, response frame and network write is covered.

The draft carrier now retains its syntax/range message reservation and the
pgwire conversion constructs a separately admitted complete frame. Its owned
message and frame copies check interruption in bounded chunks; conversion
preserves requested ERROR/FATAL severity. An owned REST encoder uses checked
JSON escaping and `Bytes::from_owner` to keep its reservation through the last
body-byte owner. The internal server's admin query execute, validate and explain
path is drafted to return this response directly, while public `route_request`
continues its legacy `(StatusCode, String)` error return and is therefore an
explicit unbounded escape. The existing unrelated REST route mappings still
need reachability audit before any broader REST claim.

All of this remains unexecuted. The draft wires the validator at binding for
Numeric-identity casts to Extended NUMERIC, including power-generated casts.
Nested Parse rejection detection and the parameter fold now pass controls and
propagate interruption/admission failures. Their semantic ordering and coverage,
non-admin REST reachability, exact-budget diagnostic overlap, and review of the
new conversion boundaries remain open. Synchronized scan/write cancellation probes are authored but
unexecuted. Do not integrate it or
claim #813 complete until these are resolved and the deferred full gate passes.

The REST encoding reservation covers the output buffer, the pinned `bytes`
owner overhead and boxed body estimate. It does not establish admission of
HTTP Response/header-map metadata or unrelated route error handling. The public
`route_request` compatibility path still returns the old bare tuple and is not
covered by that ownership claim. Exact-budget/one-byte-below overlap probes for
the source diagnostic and both transport outputs have been authored but not
executed.

A source-only reachability pass found the current REST SQL entrypoints in
`rest/query.rs` (execute, validate and explain), each routed through the admin
operation branch above. Collection vector search takes structured fields rather
than SQL, and the session SQL hit was test-only. This is not a guarantee for
future routes. The COPY input-error classifier receives direct `kind.parse_text`
errors, not the binder-owned variant; its descriptor call is therefore not a
reachable owner escape for this draft. The `pg_input_error_info` classifier receives errors from character-array
validation or runtime `cast_value`, neither of which constructs this
binder-owned variant; argument-evaluation errors propagate before that
classifier. Its independent legacy allocation behavior is not qualified here.

Pinned dependency audit: this draft's `Cargo.lock` uses `http` 1.5.0 and
`hyper` 1.12.0. `HeaderMap` exposes neither its private bucket layout nor
its actual entries-vector capacity and permits allocation beyond requested
capacity. Hyper splits response parts from the body, drains the header map,
and caches its allocation in the HTTP/1 connection for later parsing. An
owner only in the body or response extensions therefore cannot establish its
lifetime. `serve_application` can hold connection-scoped state, but retaining
one query lease per keepalive request until close would itself grow without a
bound. A separate connection-budget contract and a source-proven allocation
bound or supported accounting API are required before including header-map
storage in the query-owned diagnostic claim; no guessed multiplier is accepted.

Jev selected the narrower body-only accounting scope (1.00) while recording
headers, response metadata and legacy descriptor/public tuple escapes as
pending; the gap lead against dual body/extension ownership was 0.90. The
connection-level budget and private-allocation contract above remain a
Cassie/infrastructure follow-up, with no guessed query charge. After that
judgment, an additional body-delivery gap was found: Hyper 1.12.0's automatic
HTTP/1 strategy flattens body chunks into its own connection buffer when the
transport reports no vectored-write support. Cassie's TimedWriteTransport does
not override `is_write_vectored`, so the source Bytes owner can retire before
socket delivery. The authored body probe proves lifetime through the last
Bytes owner only, not through network write or flush. Forcing queued writes
still needs proof for accepted writes, pending flush, TLS and cancellation; do
not claim the body is admitted through delivery until that handoff is solved.

After the Flatten evidence, Jev selected the narrower Cassie-owned Bytes scope
(1.00); the lead against treating a Bytes-lifetime test as a network-write
proof was 0.94. Hyper, TLS, headers and network buffers/flush remain pending.
The unregistered, unexecuted
`docs/probes/numeric_rest_transport_pending.rs` captures the desired
pending-socket ownership invariant for the follow-up; it is not a passing test
or part of the current candidate's test modules. The compatibility-preserving
public route tuple and descriptor escapes remain unchanged.

Decision receipts are retained under the coordinator's ignored
`target/assembly-review/numeric-rest-header-final-{request,response}.json` and
`target/assembly-review/numeric-rest-transport-{request,response}.json`.
Their probabilities select the limited scope; they do not prove source parity
or replace the deferred tests.

The carrier, pgwire frame buffers and REST body reject and discard any
`try_reserve_exact` allocation whose reported capacity exceeds its admission,
following the existing query-directory convention. Rust permits the allocation
to return excess capacity before that check; strict pre-admission of arbitrary
allocator excess remains unproved and is not claimed by this checkpoint.
