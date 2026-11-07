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
unexecuted. Binary typed conversions and COPY WHERE's replacement directory
remain pending. Search-path canonicalization is bounded and admitted, while
existing `namespace_exists` and relation-resolution map scans and temporary
name allocations remain unqualified Cassie-side work.
