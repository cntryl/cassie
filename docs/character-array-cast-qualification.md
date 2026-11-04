# Character array cast qualification

This source-only #796/#807 increment implements the selected
[character array conversion law](type-contract.md#character-array-element-conversion-2026-11-04).
It is based on combined query parent `94042b92`, including array/source
corrections `12e1889d`. It changes no Midge engine, dependency version, row
encoding, storage layout or public value representation.

## Behavior and ownership

Native arrays map selected scalar elements through the existing checked scalar
cast policy, retaining dimensions, lower bounds and NULL positions. Unknown
array literals parse without a target character length before explicit casts
truncate by character count and pad CHAR. Source CHAR converted to TEXT or
VARCHAR drops insignificant trailing ASCII blanks. Bounded CHAR also uses the
existing source-aware TEXT formatter before applying width, preserving the
statement's temporal settings for scalar and array elements.

Array descriptors inherit element modifiers for explicit casts, constructors
and typed subquery/subscript expressions. Ordinary function, aggregate and
concatenation results retain their base array identity with typmod -1. The
existing UNNEST scalar descriptor law remains unchanged.

Character input validation rejects non-space overflow with `22001`; only
excess ASCII spaces may be clipped. Input inspection validates without
materializing padding. Resource, deadline and cancellation errors propagate
instead of becoming a false validity result or an error-info row. The private
array input error keeps its SQLSTATE through scalar casts, wire input and CSV
input. General implicit assignment and array-cast parity are not claimed.

The converter admits parsing, native formatting, output slots and padding
before growth. It reuses `EvaluationMemory` for generated arguments and VALUES,
returns the same owner with semantic constants, and retains a narrow owner on
ordinary projection rows containing character-array casts. Cancellation is
checked between elements and while adding padding. Original row owners and
projection search, session and subquery resolver context remain intact.

The stored canonical CHAR spelling is unchanged. Explicit recasts restore
padding from declared type metadata. A subsequent source-only increment restores
bounded CHAR-array logical padding at controlled read boundaries, before ordinary
array functions lose their result typmods. Its direct output, UNNEST and owner
regressions remain unexecuted;
this increment does not claim that wider #807 acceptance cell complete.

## Evidence and pending validation

The finite pinned PostgreSQL18.6 receipt contains 21 queries: 17 successful
results and four intentional `22001` assignment failures. It records explicit
truncation/padding, multibyte character boundaries, lower bounds, NULL/empty
distinctions, CHAR-to-text conversion, input validation, native INT/BOOL
correspondence and array OIDs/typmods. These are oracle receipts, not Cassie
test results or a PostgreSQL parity claim.

Nine wire tests, seven source-local unit tests and one SQL resource test are
authored and **unexecuted**. They include text/binary parameters and results,
multidimensional negative bounds, statement timezone correspondence, ordinary
function result modifiers, stored canonical recasts, assignment rejection and
same-session recovery. Admission probes use just-below/at-bound budgets and an
element conversion counter. Cancellation is triggered at a private synchronous
between-element hook. Constant and projection witnesses retain padding until
their owner drops. Native NUMERIC scale is admitted before formatting despite
a small final character width.

The existing `CHAR(1000000)[]` VALUES resource witness now requires
`ResourceLimit` instead of the former staged `Unsupported` rejection. Its
assertion does not accept unpadded success. A computed array projection uses
lowered budgets to distinguish denial from admission, then checks owner release
and a follow-up query.

Jev selected native element mapping, existing scalar correspondence and context
owners at 1.00, and separate input/explicit modes at 0.99. Initial review gaps
were preallocation 0.32, projection handoff candidate 0.39 and binary shape
candidate 0.30; each has a focused authored probe above. The final review
retained memory escape 0.48 and native semantics 0.56 while selecting integration
at 0.68. These probabilities remain unresolved leads until the full gate runs;
they are not reduced by another review. The array function descriptor correction
uses pinned `exprTypmod` source and Jev's selected normalization law (0.81).

Per the user's deferred-run instruction, no Cargo build, Cassie test, Clippy or
test-policy command ran for this increment. Rustfmt parsed/formatted the touched
Rust files, and `git diff --check` passed. The complete ordered validation and
expanded oracle comparisons belong to the frozen combined bundle. No issue
closure, hosted CI, merge or support promotion is claimed here.

Raw primary sources, oracle output and Jev requests/responses are preserved in
the isolated worktree's ignored `target/v020-evidence/character-arrays/` without
credentials.

## Stored logical values: pending native qualification

The finite PostgreSQL 18.6 source/character oracle now records 54 expected
outcomes: 46 array/source cases and eight final character cases. These include
stored negative lower bounds, padded CHAR element binary bytes, session-aware
scalar/array conversion, ordinary function/aggregate typmods and quoted-tab
input rejection. A separate final stored-logical batch records eight more
expected outcomes for direct output, reverse, UNNEST, RETURNING, ordering, exact
recasts, concatenation and aggregation. These are oracle receipts, not Cassie
test results.

The private controlled hydration helper computes every replacement string bound
without allocation and reserves the complete padding before changing an element.
It retains a separate operator lease through the row, including its pending lookup;
a row arriving without a source lease also admits its existing body before that
operator owner is attached.
Typed and encoded column paths retain their original scratch reservation until
the destination has admitted a copy. Cancellation is checked during sizing,
between elements and at each 4,096-character padding chunk. Durable encoding and
Midge remain unchanged. Unbounded bpchar and non-character arrays retain their
existing behavior.

The source-only caller ledger covers ordinary full/projected scans, typed and
encoded transport, portal pages, point/scalar-index reads, bounded joins, hybrid
retrieval, ordered row-id and heap top-k reads, time-series reads, ordinary
RETURNING, and conflict existing-row evaluation. Ordered rows retain declared
array identity across aliases and keep candidate/source leases in their returned
row. The time-series row converter was extracted to keep the parent under its
file-size limit. Same-bound bounded CHAR-array casts no longer bypass element
conversion in the rich source-argument evaluator.

Five hydration-helper regressions, one ordered-owner regression, one rich
exact-cast regression and five wire regressions are authored but unexecuted. Static test-policy checks and Rust formatting are
separate evidence; build, complete tests, strict workspace Clippy, final inventory
and integrated native qualification remain pending under the user's batch gate
instruction.

The legacy `ALTER COLUMN ... USING` caller now has a separate source-only
controlled logical-input increment described below; its native qualification
remains pending. OLD/NEW row images now
have a separate source-only controlled preparation increment described below;
its native qualification is pending. The mixed-NULL COALESCE descriptor correction is authored for already-admitted
equal array operand types: an untyped NULL removes the result typmod, while a
matching typed NULL retains it. One binder and two wire regressions are unexecuted.
Different-width common-type acceptance and wider array conditional behavior remain
pending; the separate NULLIF/GREATEST/LEAST finite contract still excludes arrays.

The logical hydration checkpoint follows `dc52c54d`. Jev selected the controlled
logical boundary at 1.00. Its admission 0.88 and identity-elision 0.50 origin
leads produced the authored bound/counter and rich-cast regressions. The first
implementation review retained an identity concern at 0.42; the complete source
route was then supplied for adversarial review (character conversion 0.90),
with the native regression still pending. Row-owner, chunk-cancellation,
time-series preflight and top-k lifetime gap judgments were 0.25, 0.16, 0.15
and 0.22. Source audit also retained pending lookup admission and row-id page
reservations before freezing the checkpoint. These judgments are review leads,
not native execution or acceptance evidence.

## Controlled OLD/NEW preparation: pending native gate

The next checkpoint corrects the earlier caller ledger: `198949b7` wired the
EXISTS RETURNING path; ordinary array RETURNING still used a separate branch.
Both now admit their raw input rows before adding metadata, native character-array
conversion or padding. Virtual returning schemas are cloned under admission;
exact qualified field keys take precedence when assigning each row's types.
Rows are typed independently because MERGE INSERT and UPDATE/DELETE shapes can
carry different reserved identity positions. A separate metadata reservation
follows each row through projection and managed result admission.

The existing OLD/NEW builders keep their canonical storage envelope. Controlled
preparation estimates native envelope conversion and every padding string without
allocation, then reserves the complete bound before decoding or growing output.
The stable storage envelope, durable CHAR canonicalization and Midge remain
unchanged. The finite PostgreSQL context batch has nine expected outcomes and a
separate final batch has two more for EXISTS projection and mixed MERGE actions;
these are oracle evidence, not Cassie test runs.

Seven wire and two source-local regressions are authored and unexecuted. They
cover joined UPDATE/DELETE, INSERT/conflict, MERGE with mixed actions, ordinary
and EXISTS projections, binary lower bounds, NULL images versus NULL elements,
exact decode admission and virtual type/owner lifetime. Jev retained decode
admission 0.30, metadata ownership 0.56 and virtual EXISTS schema 0.37 leads.
The authored decode counter, Weak reservation-owner assertion and EXISTS wire
projection are their focused probes; these probabilities are retained without
substituting a later score for native execution. Complete compilation, native
semantics/resource/cancellation checks and strict workspace Clippy remain pending.

## Existing NUMERIC USING target: pending native gate

The controlled USING checkpoint selects logical CHAR-array input for the existing
row-authoritative TEXT-to-NUMERIC(20,2) target. PostgreSQL 18.6 records 3.00 for
`octet_length(items[-2])`, 4.00 after a CHAR(4)[] recast and 1.00 after a TEXT[]
cast, with NULL arrays producing SQL NULL. A final three-case receipt records
these outcomes. This is oracle evidence; the three wire regressions are unexecuted.

USING cast provenance now binds against the existing per-operation working schema,
including qualified references. Runtime controls flow through ALTER and its
multiple-action calls. The existing controlled Midge row reader retains each
input through row construction and evaluation. Logical padding, argument scratch,
native formatting and converted payloads are admitted before growth. The returned
AccountedVec owns prepared payloads after evaluation; both payload and write-slot
reservations remain live through the existing rewrite mutation sequence. Every
payload and write-vector slot is prepared before the first mutation begins.

Four private regressions are authored for output retention, padding denial,
cancellation after conversion and cancellation at the prepared rewrite boundary.
The latter observes both retained reservations before mutation and requires the
original row to remain unchanged. It is the focused probe for Jev's retained
output-owner lead (design 0.55, review 0.45); neither score is native evidence.

This uses existing Cassie-side APIs over published Midge 0.5.1 and changes no
durable layout or CHAR canonicalization. It does not qualify additional USING
target pairs, staged-row source semantics or full rewrite failure/recovery.
Those remain pending under #839 and its #833 atomic-publication dependency;
the existing mutation sequence and its broader storage-error behavior are not
reclassified as complete by controlled preparation. Compilation, native tests,
strict Clippy and the final combined inventory remain pending.


## Frozen integrated gate v3: source corrections pending rerun

The integrated `86174ceb` gate ran 9,988 passing tests, 67 failures and 12
ignored tests. This lane retains its actual failing CHAR/VALUES, array codec,
COPY, series-sublink and projected-memory observations. The subsequent source
corrections are unexecuted; none is recorded as native green.

A same-family CHAR coercion to an unconstrained common type must retain each
value's existing padding. Its previous path converted through TEXT and removed
those blanks. Source CHAR to TEXT/VARCHAR still removes trailing blanks, and
bounded explicit CHAR casts still truncate/pad. The general ARRAY constructor
parser now handles both postfix and CAST syntax instead of the obsolete
TEXT-literal-only shortcut, preserving typed element expressions. Right-side
implicit-lateral table-function sublinks inherit the left relation's visible
schema while ordinary non-LATERAL subqueries retain their isolated scope.
Two scalar cast, one parser and one sublink-typing regressions were authored
before these runtime corrections; the frozen integration failures remain the
executed red evidence.

PostgreSQL 18.6 confirms the finite VARCHAR(4)[] descriptor typmod is 8; the
matrix's previous -1 expectation was stale. COPY over-width CHAR-array input
uses SQLSTATE 22001, matching the existing TypeInput error route; integer input
still checks Parse. Codec payload and COPY no-publication assertions are retained.
The timezone test now executes SET on the wire connection being queried rather
than a separate seed session. The computed character-array owner test disables
the execution-result cache and drops its recovery result before requiring zero
retained query memory, while observing positive admission for its live result.

The earlier 198949b7 logical-hydration and exact-cast checkpoint addresses the
frozen stored-array recast failure; d8e10c29, 8feeb0c6 and 72445740 retain their
separate authored NULL-modifier, OLD/NEW and controlled-USING coverage. Their
compilation, behavior, admission and cancellation results remain pending.
Jev selected the same-family cast and general-constructor corrections at 1.00,
with 0.88 support for the missing left typing scope and 0.72 for the fixture
corrections. The seven-case pinned oracle batch and bounded review requests
are retained in ignored target evidence. Complete native validation and strict
workspace Clippy are reserved for the next frozen integrated gate.

The implementation review retained gap judgments of 0.26 for casts, 0.19 for
constructor parsing, 0.25 for lateral scope and 0.22 for fixture laws. A separate
compile/API lead at 0.48 was followed by a bounded exact-declaration review and
Rust syntax/test-policy probes; it identified no concrete supported defect
(choice none 0.87). This does not replace the deferred native compiler/Clippy
gate, and the original 0.48 lead remains in the evidence record.
