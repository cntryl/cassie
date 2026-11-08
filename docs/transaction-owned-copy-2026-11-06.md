# Transaction-owned COPY FROM owner (#833, #826)

COPY FROM in an explicit transaction binds a single admitted command owner at
entry. That owner carries a transaction-generation and target-identity fence,
the descriptor/catalog including the transaction's own DDL, and the owning
same-database-family storage scope. CopyInResponse column descriptors, text,
CSV and binary decoding, default and constraint evaluation, and statement
batch ingestion use that same owner through CopyDone. Do not re-resolve a name
against published Main after accepting the stream. If the originating
transaction/generation, target identity, catalog authority or storage overlay
has changed before the atomic fork capture, fail COPY before any row or sequence
side effect. A parent mutation after capture makes fork publication fail without
publishing rows or redirecting the target; sequence values actually consumed by
the failed statement remain nontransactional, matching PostgreSQL. Session
recovery and cancellation remain unchanged. Existing published COPY targets and unaffected
COPY after unrelated transactional DDL remain accepted.

The selected route reuses Cassie's admitted catalog snapshot/descriptor owner,
query interruption and deadline authority, bounded COPY input buffers,
statement mutation fork and transaction-owned sequence cursor. The owner and
all retained descriptors remain charged through the final COPY worker or
connection retirement. Rows and catalog/storage effects publish atomically in
the same existing Midge database family; rollback, savepoints, failed rows,
resource admission and cancellation do not expose partial rows. The current
published Midge 0.5.2 APIs and existing Cassie storage overlay must be checked
for each required operation; no new Midge source, storage abstraction,
persistent representation or cross-database transaction is selected. Any
unavailable prerequisite remains a named pending blocker, not a silent fallback.

PostgreSQL 18.6 reference observations: inside BEGIN, creating a SERIAL table
and copying alpha/beta returns `COPY 2`, exposes rows `(1,alpha),(2,beta)`,
sets currval/lastval to 2 and after commit advances to 3. A COPY with first
valid row then invalid integer returns `22P02`; rollback to its savepoint leaves
zero rows, while the next sequence value is 2. Dropping a published table,
recreating the same name with a different schema and copying into it targets
the replacement; rolling back restores the original row. Raw oracle receipt:
`target/scalar-oracle/staged-copy-owner.txt` in the temporal worktree.

Jev selected the single pinned owner with probability 1.00. Its 0.87 gap
requires focused text/CSV/binary, same-name drop/recreate, owner-generation
change between CopyInResponse and CopyDone, staged defaults and sequence
history, unrelated DDL, cancellation/error protocol recovery and same-family
publication probes. Before implementation, author subsystem-specific
regressions with these properties; they remain unexecuted until the combined
local validation gate. This increment alone does not complete #833 or #826.

Jev selected this timing distinction (0.95): validation and fork capture are
one transaction-locked step, while a concurrent change after capture is caught
by the existing fork publication fence. Its 0.77 gap requires a synchronized
post-capture parent mutation probe that checks zero row publication, no target
redirection, preserved nextval for a still-live identity, recovery and
cancellation. Receipt `target/assembly-review/staged-copy-sideeffects-*.json`.
The immutable storage snapshot compares all staged keys, values, expected
preimages, TTLs and epoch selections with interruption checks; cursor values
remain deliberately shared/nontransactional and in-flight pairs stay leased.

The pinned owner lives for the complete input stream, so its initiating query
deadline also bounds stream retention. Jev selected (0.99) an idle-tag deadline
that returns the existing `DeadlineExceeded` error, and a connection close when
the deadline expires after any frontend frame tag has been consumed but the
frame is incomplete. Closing releases the owner without emitting a response into
a partially consumed frame. Existing explicit CancelRequest handling continues
to finish an already admitted frame. Required unexecuted probes cover idle
error/recovery, partial length and body stalls/close and owner release, a frame
completion concurrent with expiry, CancelRequest overlap, and timeout disabled;
the Jev gap was 0.81. Receipt: `target/assembly-review/copy-deadline-*.json`.

When readiness and expiry are both observable in the same poll, a complete
frame takes precedence over the deadline; a ready explicit cancellation takes
precedence over an idle tag. After the tag is consumed, expiry closes even if
an explicit cancellation is also pending, since cancellation cannot safely
reply until the incomplete frame has been consumed. With the timeout disabled
there is no timer, and the existing cancellation and frame semantics apply. The
idle deadline maps to SQLSTATE 57014 through the ordinary deadline descriptor.

The retained COPY input buffer is admitted to the same request tracker as the
owner before allocation, including simultaneous old and replacement capacities
when it grows; it remains admitted through decoding and ingestion and releases
on success, failure, timeout, or connection retirement. The preexisting wire
reader separately allocates each received frontend frame before handing its
body to COPY, and format/encoding decoding can allocate further scratch and
rows; this increment does not claim those independent allocations are covered.
A live stream also rejects if any of the compared catalog maps changed, even
for unrelated concurrent catalog DDL; catalog changes completed before binding
remain supported. Exact target-specific relaxation remains pending until all
write dependencies and storage-only mutations can be proven equivalent.

Jev selected (1.00) same-tracker admission of the complete wire-frame body
before allocation. If admission fails after the length has been consumed, COPY
drains exactly the remaining frame with fixed scratch before reporting 54000,
so the next frontend message stays aligned; EOF or deadline during that drain
closes the connection. Explicit cancellation still waits until the admitted
frame or rejected frame's drain completes. The returned body reservation stays
live while it overlaps the accumulated stream buffer. CopyFail's temporary
string is admitted as an overlapping decode allocation. Gap 0.87 requires
synchronized admission-denial, drain/recovery, cancellation/deadline, malformed
length, and owner-release probes. Receipt:
`target/assembly-review/copy-frame-admission-*.json`.

Jev selected (0.99) a physical identity fence for catalog descriptors whose
storage ID is zero: pin the actual `RowSchema.relation_id` under the collection
write gate using the originating transaction's storage overlay, then revalidate
that physical ID under the same gate before fork capture and retain the gate
through publication. A zero or absent physical ID cannot prove identity and
must fail closed. Raw public embedded Midge drop/create and
`Cassie::register_collection` can replace storage while recreating an identical
zero-ID catalog descriptor; catalog equality alone is insufficient. Probe
same-name raw recreation, change at the bind/fork boundary, repeatable-read
versus staged descriptor authority, legacy no-ID, interruption and admission;
these probes remain unexecuted. Receipt:
`target/assembly-review/copy-physical-id-*.json`.

The identity read checks the captured same-family storage overlay first, then
a fresh native read of the published row descriptor; a staged tombstone is
absent and a descriptor without `relation_id` fails closed. Decode uses a
bounded checked reader and admits scratch. The Midge `get` result is obtained
before its byte length can be reserved; this pre-existing API overlap remains
separately pending rather than being represented as fully admitted.

Catalog/storage snapshot and the first physical-ID pin must be captured while
holding the same collection gate. A target may be provisionally resolved to
choose the gate, but must be resolved again from the gated snapshot; a changed
canonical target causes rejection before CopyInResponse. Merely comparing the
catalog version is insufficient: the public Midge API can recreate physical
storage without changing the catalog. A synchronized replacement between
provisional resolution and capture must either wait for the gate or cause a
stale-owner rejection before the stream starts.

LATIN1 transcoding and explicit UTF-8 validation use the initiating request's
controls, charge the exact retained transcoded capacity before allocation, and
check interruption while scanning and copying in bounded chunks. Text, CSV and
binary row/cell decoding still needs its own admitted owners and bounded
interruption before this increment can be called resource-complete. Published
Midge 0.5.2 `Transaction::get` provides only a synchronous whole-value result;
it exposes no length preflight, caller buffer, admission or cancellation hook,
so native descriptor pre-read admission is an upstream API limitation.

Jev selected capacity-coupled, query-admitted row/cell owners for each text,
CSV, and binary parser, with separately admitted typed-conversion scratch
(0.71); it rejected an unproved global payload multiplier. Focused unexecuted
coverage must preserve NULL/default/quote/encoding/on-error behavior, typed
binary expansion and malformed-error precedence, and cover cancellation and
input/frame/output overlap. The COPY WHERE output directory also overlaps
parsed input and must remain charged. Gap 0.92 requires those probes before
any decoder is claimed complete. Receipt:
`target/assembly-review/copy-decoder-*.json`.

The current source draft adds admitted TEXT and shared wire/embedded CSV
row/cell owners; their leases persist through ingestion and their probes remain
unexecuted. Binary typed conversions and portions of COPY WHERE scratch remain
pending. Search-path canonicalization is bounded and admitted, while
existing `namespace_exists` and relation-resolution map scans and temporary
name allocations remain unqualified Cassie-side work.

A COPY frontend tag followed by a malformed length below four or a length above
the frontend frame limit closes the connection without ReadyForQuery: the body
boundary cannot be trusted or would exceed the configured framing limit, so
recovering the connection would reinterpret unread bytes as another message.
Valid frames rejected for query-memory admission are still drained before 54000.

The COPY WHERE accepted-row directory is separately admitted before allocation
and remains live through ingestion while the original decoded-row admission
continues to cover retained cells. Predicate compilation and some per-row
conversion scratch remain separate pending allocations. This bounded
increment does not complete COPY WHERE resource accounting.

Each COPY WHERE row's parameter vector and copied string values are separately
admitted while the decoded input and accepted-output directories remain live.
Fixed-width scalar parsing, SQL expression compilation/evaluation and its
result buffers remain governed by their existing execution controls; unbounded
text parsing and identifier/predicate compilation need their own interruption
proof and are not claimed complete by this increment.

COPY WHERE predicate evaluation must use the initiating COPY request's original
cancellation handle, deadline and memory tracker; per-row `execute_sql` creates
fresh controls and is not sufficient. The selected Cassie-internal route calls
the existing SQL execution core with the retained controls. A synchronized
sleep-entry cancellation probe must observe cancellation inside predicate
execution, rather than merely between rows; it remains unexecuted.

The nested predicate preserves the previous per-row query-admission permit,
metrics and transaction-failure handling, while `execute_sql_core` receives the
same COPY controls and publication scope rather than a new tracker or deadline.
Session `statement_timeout` tightens the initiating COPY controls before capture
and stream handling. The exhausted/available query-slot probe remains unexecuted.

Binary COPY now admits row and row-directory capacities and retains each output
String's observed capacity through ingestion. The existing `binary_to_value`
and `value_to_text` codecs allocate and may scan before those output reservations,
and column metadata lookup may clone schema before admission. Typed binary
conversion pre-admission, interruption, and full input/value/output overlap
remain pending; this incremental directory accounting is not binary resource
qualification and is distinct from the Midge native-get API limitation.

Jev selected direct, admitted and chunk-checked binary conversion for OIDs 25,
1042, 1043 and 17 (1.00). The first three preserve the text codec's UTF-8 bytes;
bytea emits `\\x` followed by lowercase hex. All other accepted OIDs retain the
existing codec path and are explicitly pending scratch admission/interruption.
Gap 0.76 requires parity probes for split/invalid/truncated UTF-8, empty and NUL
text, bytea edge bytes, typmod, exact/below budgets, input/output overlap,
frame/trailer/NULL ordering and synchronized cancellation. No tests run.
Receipt: `target/assembly-review/copy-binary-text-*.json`.

Jev selected (1.00) direct admitted stack-decimal output after the existing
fixed-width binary decoder for OIDs 16, 20, 21, 23 and 26. Preserve the existing
nonzero bool behavior, signed integers, unsigned OID, short/excess error order,
and NULL/frame/trailer handling. Gap 0.81 requires all width boundaries,
i64 min/max, u32 max, bool 0/1/ff, malformed sizes, exact/below budgets,
input/output overlap and cancellation; probes are unexecuted. Float, JSON,
array, vector and extended codecs retain existing behavior and remain pending.
Receipt: `target/assembly-review/copy-binary-fixed-*.json`.

Jev selected direct UUID binary conversion (OID 2950, 0.96). Valid 16-byte
input is rendered in a fixed stack buffer and copied once into an exact-capacity
admitted output. Invalid short input and overlong input use the original codec
on the short slice or first 17 bytes respectively, preserving invalid versus
excess diagnostics. Gap 0.80 requires all short lengths, 16/17/larger lengths,
zero/ff/mixed lowercase hyphenation, NULL/trailer order and exact/below-budget
owner lifetime probes. All probes remain unexecuted. Receipt:
`target/assembly-review/copy-binary-uuid-*.json`.

Jev selected (0.95) bounded NAME binary conversion (OID 19): preserve the
existing 63-byte and UTF-8 receive rules, including length rejection before
UTF-8 and the outer codec's 64-byte prefix; copy a valid name once into an
admitted owner. Gap 0.70 requires UTF-8 boundaries, NUL, invalid/64+/short
inputs, exact/below budget and lifetime probes; they remain unexecuted. Other
binary codecs remain pending. Receipt:
`target/assembly-review/copy-binary-name-network-*.json`.

Jev selected (0.95) direct fixed-stack output for binary macaddr (OID 829),
macaddr8 (774) and pg_lsn (3220) after the existing binary decoder validates
width and error order. MAC output uses lowercase colon-separated pairs; pg_lsn
uses unpadded uppercase high/low 32-bit words. Gap 0.59 requires zero/ff/mixed
MAC, zero/one/high-bit/max LSN, short/excess widths, exact/below budgets and
lifetime probes. These probes remain unexecuted. Other codecs remain pending.
Receipt: `target/assembly-review/copy-binary-name-network-*.json`.

Jev selected (0.99) direct admitted binary inet/cidr output (OIDs 869 and 650)
after the original binary decoder validates the input. A fixed stack formatter
must preserve existing IPv4/IPv6 compression, equal-run priority, mapped and
compatible output, and inet/cidr mask rules; output and its lease transfer
together into the row. The maximum address spelling is 39 bytes plus `/128`.
Parity (0.91), validation/error precedence (0.53), stack bound (0.30) and lease
lifetime (0.40) require focused source checks and probes. Malformed diagnostic
scratch and all other unaddressed binary codecs remain pending; these authored
probes have not been executed. Receipt:
`target/assembly-review/copy-binary-inet-{request,response}.json`.

Jev selected (0.92) direct admitted binary REAL output (OID 700) using the
original decoder and stack-backed Rust scientific formatting followed by the
same `-4..6` exponent threshold and digit placement as `real::format_text`.
Keep signed zero, infinities, NaN, short/excess binary error ordering and the
output lease handoff. Formatting parity (0.87), original validation (0.35)
and owner lifetime (0.39) require focused source checks and probes. Float8,
malformed diagnostic scratch and all other unaddressed binary codecs remain
pending. Probes are unexecuted. Receipt:
`target/assembly-review/copy-binary-real-{request,response}.json`.

Jev selected (1.00) a shared stack-backed FLOAT8 formatting core for binary
COPY OID 701, preserving the existing String adapter and exact shortest,
midpoint, exponent and special-value behavior for all callers. Replace the
midpoint mantissa's temporary String with checked digit folding. Parity (0.90),
existing callers (0.83), parser allocation (0.66), owner lifetime (0.53) and
digit-fold semantics (0.32) require focused source checks and probes. The
observed Rust toolchain's formatter and decimal parser use stack-backed paths;
this is toolchain-specific evidence rather than a guarantee for every possible
Rust implementation. Error diagnostic scratch and unrelated codecs remain
pending. Probes are unexecuted. Receipt:
`target/assembly-review/copy-binary-float8-{request,response}.json`.

Jev selected (0.98) direct admitted binary BIT/VARBIT output (OIDs 1560/1562):
check the signed bit length and exact `ceil(bits/8)` payload first, then emit
high bits in order and ignore unused trailing padding, checking interruption
at bounded intervals. The OID codec's typmod is None; malformed input may use
the original codec only on branches that reject before allocating bit storage.
Parity (0.80), malformed fallback (0.78), error order (0.71), interruption
(0.55) and lease lifetime (0.43) require focused source checks and probes.
Other codecs and malformed diagnostic scratch remain pending. Probes are
unexecuted. Receipt:
`target/assembly-review/copy-binary-bits-{request,response}.json`.

Jev selected (0.99) direct admitted binary TIME output (OID 1083) after the
existing decoder's width, range and error checks. A checked stack clock must
match `format_time`, including fractional zero trimming and `24:00:00`;
output and reservation transfer together. Formatting parity (0.80) and owner
lifetime (0.51) require source checks and focused probes. Other temporal OIDs
and error diagnostic scratch remain pending. Probes remain unexecuted. Receipt:
`target/assembly-review/copy-binary-time-{request,response}.json`.

Jev selected (0.99) direct admitted binary DATE output (OID 1082), retaining
the original decoder's width, range, infinity and error checks. Existing
`value_to_text` calls `TypedValue::text()` with the default ISO session; the
checked stack renderer must preserve that ISO year/month/day and BC era output.
Parity (0.78), owner lifetime (0.55), decoder errors (0.41) and default-session
selection (0.38) require source checks and focused probes. Other temporal OIDs
and malformed diagnostic scratch remain pending. Probes remain unexecuted.
Receipt: `target/assembly-review/copy-binary-date-{request,response}.json`.
