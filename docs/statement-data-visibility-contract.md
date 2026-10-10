# Statement Data visibility contract

## Selection and implementation status

[#763](https://github.com/cntryl/cassie/issues/763) selects one shared
same-database Data statement view (choice B), following the user decision.
This is the implementation contract, not a claim of completed support.
At baseline `0a36504091287303b390182a031571793ea66f1f`, separately
opened read owners acquire separate Midge transactions. End-to-end sharing,
race qualification and release acceptance remain pending. Prerequisite
[#755](https://github.com/cntryl/cassie/issues/755) is closed.

## Acquisition and visibility

After binding and authorization, before the first authoritative Data read,
a statement captures one readonly Midge transaction for its logical database
and one immutable copy-on-write snapshot of its session's staged writes.
Every selected read owner in that database uses those same captured owners.
A commit after capture cannot change the statement's committed rows, index
membership or Data metadata; a staged-write change after capture cannot
change its overlay. Read-path eligibility checks use that captured map, including
an empty captured overlay; another session keeps its live staged-write authority.
Mutation staging and commit checks retain their existing live state.
Empty inputs obey the same acquisition law. A subsequent
statement captures a fresh view and can observe later committed state.

Existing embedded executor entrypoints use the same acquisition law when called
directly. An internally supplied matching owner must be installed for that
execution; an ordinary direct call captures a fresh local owner before its first
Data read. Reusing caller controls alone does not select transaction-wide
repeatability. Command mutation authorities remain outside the read scope.

This law covers row sources, joined sources, repeated CTE and subquery reads,
worker execution, index candidate and row retrieval, column metadata and
chunks, specialized retrieval, and their scalar or row fallbacks. A fallback
must not silently open a newer Data view. Existing ordering, equality,
visibility, authorization and descriptor laws remain authoritative.

The view covers exactly one logical database's Data physical family.
Schema/catalog and Temp visibility remain separate. Cross-database
synchronization, transaction-wide repeatable snapshots, REPEATABLE READ,
SERIALIZABLE and transactional DDL are not selected. Ordinary bound SQL relations in another database remain rejected by the
existing binder boundary. Internal adapter or direct-plan foreign-family reads
retain fresh reads outside this same-database guarantee.
Midge remains the only storage layer; no persistent layout, dependency
upgrade or public API change is selected.

## Mutation and commit authority

DML acquisition is limited to source materialization: INSERT SELECT, UPDATE
matched rows and predicates, and DELETE matched rows and predicates. Their
joins and EXISTS reads share that owner. The source scope ends before mutation
preparation, uniqueness/FK checks, write staging and commit validation, which
keep their existing authorities.

A DML statement's read-source selection uses the statement view. Its private
mutation staging, constraint checks and write transaction retain their
existing authorities. A statement read owner must never substitute for the
fresh gated commit state used to validate foreign keys and parent references
(TX-08). Existing mutation rollback, multi-collection atomicity, failed-block,
savepoint, session-setting and post-durable maintenance laws (TX-01 through
TX-09) remain required. Read sharing cannot hide writes from the existing
same-statement mutation or constraint machinery.

Subqueries in `INSERT`, `UPDATE` and `DELETE` `RETURNING` expressions use the
same pre-command Data view and immutable snapshot of prior transaction writes
as other DML source reads. That view excludes writes staged by the current
command. Correlation still supplies the command's returned row: the inserted
or updated new row, or the deleted old row. Every row produced by one
multirow command uses the same captured view; a later command captures a fresh
view and can observe the earlier command's committed or staged writes. Install
this view only for `RETURNING` source reads, keeping mutation preparation,
constraint checks, staging and commit under their existing authorities.

`RETURNING` remains atomic under the existing publish-after-complete-success
wrapper. If expression evaluation fails, the command must preserve its current
rollback, failed-transaction and savepoint behavior and must not publish a
partial result or partially staged command. Lazy scalar selection, exact
descriptors, admission, cancellation and owner retirement remain required.
This finite selection does not establish uniform behavior for every DML family
or expression form beyond the separately qualified cases.

## Portals and termination

Parse, Bind and Describe alone do not capture a Data statement view. The
first execution captures the view and overlay. A suspended cursor retains
them; a materialized portal retains its immutable result. Resume must not
reopen the cursor's Data transaction or recapture the overlay. Existing
commit, rollback, Close, statement-close cascade, disconnect and implicit
segment boundaries terminate owners; explicit Sync preserves live explicit
transaction portals as described in the
[wire transaction contract](pgwire-transaction-contract.md).

Cumulative row, memory, deadline and cancellation limits retain their current
portal authorities. Completion and error release newly owned rows, workers,
reservations and snapshot pins at their documented termination boundaries.

## Caches and specialized artifacts

Cache hits must describe the captured view, not the current live Data state.
Data epoch and collection generation are stored in the same Midge Data
transaction as their mutations. Their values must be read from the captured
transaction when used as snapshot cache identity. Existing schema,
authorization, parameter and setting cache checks remain required.

A specialized artifact or cached representation is usable only when its
existing validity fence is compatible with that view. Otherwise use a
controlled existing fallback over the same captured Data rows, or preserve
the existing explicit unsupported/error boundary. This selects no new
fulltext, vector, graph, hybrid or column codec support. Current artifact
metadata cannot silently select newer rows during fallback.

## Resource and ownership contract

Readonly owners are shared privately through reference-counted ownership;
Midge owns MVCC version retention and snapshot pins. Cassie admits its own
owner metadata and retained staged-overlay backing before construction or
capture and keeps their reservations for the actual owning lifetime. Worker
clones share the original owner rather than deep-copying session state.
Existing row, cursor, artifact, output and worker allocation admission laws
remain required. Sharing a transaction does not excuse fresh output backing
from admission or allow cancellation to release charge before live buffers.

## Finite qualification matrix

| Obligation | Required oracle | Current disposition |
| --- | --- | --- |
| TX-01 through TX-06 and TX-08 through TX-09 | Reuse mapped mutation, commit-gate, recovery, settings and savepoint evidence; add only uncovered controls | Fourteen exact mapped mutation, visibility, savepoint, settings, atomicity and fresh-FK witnesses replayed 14/0; additional context qualification remains separate |
| TX-07 | Captured whole-session COW overlay survives subsequent session writes and cursor resume | Captured staged rollback and statement-wide owner witnesses pass; suspended cursor rollback/resume passes |
| TX-10 joined sources | Commit barrier between empty, partial and multirow source reads returns only the captured version | Current focused source witnesses pass: multirow JOIN, empty acquisition and visible right rows after an empty left read; broader path qualification remains pending |
| CTE, subquery and workers | Repeated reads reuse the same transaction and captured overlay | Repeated CTE consumers and derived-subquery join witnesses pass; CTE materialization does not prove a second raw read; eight existing conversion controls replayed successfully, including two active workers cancelled with zero charge/workers and reusable permits; a 1026-row captured statement dispatched two scan workers after committed/staged divergence; existing scoring, grouped aggregation and aggregation-timeout controls replayed 3/0 on rebased source |
| Index and column paths | Candidate IDs, rows, metadata, generation and controlled fallback use one view | Captured staged-overlay rollback controls pass for scalar indexes, ordered reads, column summaries, analytical projections and projected batched scans; six scalar/column/fulltext/vector/hybrid/projection committed-warm barriers and latest-authority graph mutation barrier pass; scalar/fulltext candidate removal and vector score-change UPDATE controls pass |
| Specialized paths and caches | Existing artifact fence or compatible cache identity; fallback stays on captured rows | Fulltext statistics epoch and staged-cache controls pass; persisted fulltext, HNSW and hybrid overlay rollback controls pass; time-series overlay rollback controls pass; six scalar/column/fulltext/vector/hybrid/projection committed-warm barriers and graph mutation barrier pass; time-series committed bucket barrier passes with four native hits and no fallback |
| Portals | First-execution membership, values, order and overlay persist across resumes; termination releases owners | First Execute after Bind observes intervening commit; suspended savepoint rollback and Close controls pass; existing completion, cancellation, cap and concurrent-write controls replay; automatic source-denial owner release/retry and portal lifecycle controls pass; full ownership acceptance remains pending |
| Resource failures | Admission denial and cancellation preserve owner/row lifetime; cleanup releases owned charge | Combined-owner lifetime, automatic-owner source denial/retry and active conversion cancellation pass; complete repository qualification remains pending |

Qualification must retain exact source, inputs, results and command provenance.
A passing selected path does not promote another path. Keep #763 open while
any selected acceptance is pending, failed or blocked. Complete repository
validation, independent adversarial review, exact-head hosted checks and
merged readback remain required before closure.

Current focused witnesses live in
`src/app/statement_read_tests/sources.rs`. Their barriers assert an independent
session committed after capture or between source materializations, compare the
captured rows and values, and then verify a fresh statement sees the commit.
These tests qualify existing implementation behavior; they establish no new
pre-repair regression. The final source-only probe run passed 24 statement-view
tests, including the observable RIGHT JOIN empty-input probe required after
Jev returned an empty-witness gap probability of 0.37. Full acceptance remains
pending as described above.

The exact fulltext fallback now keys Temp corpus statistics to the matching
captured Data owner's epoch, including engine, collection database and supplied
session identity checks. The specialized `_id` score projection established
RED (0/1): an old captured row scored 1.492780 from the newer warmed corpus
instead of 0.287682 from its original corpus. The repaired focused statement
suite passed 27/0, including statistics-cache hits, next-statement fresh scores
and cross-engine/session supplied-owner rejection. The generic `id` projection
control passed before repair but bypassed this statistics-cache path; it is not
evidence of that path's correctness. Other artifact paths remain pending qualification.

The staged-corpus statistics witness also established RED (0/1): a ten-row
staged corpus reused the one-row committed corpus statistics (0.287682 versus
1.492780 for an equivalent committed reference corpus). Shared Temp statistics
lookup and storage are now bypassed when the captured collection overlay has
changes. Context construction still uses exact captured rows. The focused
statement suite passed 28/0; other-session and post-rollback committed scores
remain unchanged. Jev's 0.15 staged-cache gap judgment was contradicted by this
actual probe and is retained as a false negative. The subsequent native eligibility qualification is recorded below.

Nine native read witnesses established semantic RED (0/9) when a statement
captured staged writes and a nested ROLLBACK subsequently cleared the live
session. Persisted fulltext top-k and filtered reads, scalar indexes, ordered
reads, column summaries, HNSW, hybrid, analytical projections and the projected
batched scan helper omitted captured staged rows or values. Ten private read
eligibility gates now use the existing captured-overlay accessor; the projected
batched helper merges its borrowed captured staged snapshot. Matching captured
empty overlays dominate live state, while absent or mismatched session owners
retain the existing live fallback. Fresh mutation and referential checks remain
unchanged. The focused statement suite passed 37/0. The subsequent time-series range witness passed (1/0): captured rows 10/20
survived nested rollback, the next statement returned 10, and metrics recorded
two native range hits and two session-changes fallbacks. Jev returned
captured-empty and borrowed-merge gap probabilities of 0.16 and 0.21; independent
source review accepted the eleven-file runtime patch. These results do not
complete committed artifact-generation barriers, portal/worker qualification
or repository-wide acceptance.

The existing conversion resource suite replay passed 8/0 on the current source,
including active parallel cancellation, denied-permit serial fallback and
source/output reservation overlap. Source review shows conversion, aggregation
and scoring workers consume preloaded inputs rather than acquiring Data inside
those closures; Jev returned a newer-worker-read gap probability of 0.09. This
resource replay does not establish whole-statement dispatch or portal ownership.
Jev selected first-execution and suspended-portal boundary probes as the next
qualification, with 0.97 confidence; those probes remain pending.

The new finite wire probes passed 3/0: a commit between Bind and first Execute
is visible at execution; a suspended cursor resumes its original remaining
rows after staged writes and savepoint rollback; Close removes a live suspended
portal. Existing completion controls replayed 5/0, and four exact cumulative
cap, cancellation and concurrent-write controls passed. Global query-memory
metrics do not report the separate portal retention controls, so an initial
positive-global-charge assumption is preserved as an invalid test oracle.
The existing retained-budget lifecycle fixture initially rejected its first
page at 262656 bytes against 262144 before reaching its intended retention
assertions. With two additional KiB for admitted statement owner/source overlap,
its unchanged three-held, fourth-denied and Close/replacement assertions passed.
This changes only the test fixture, not production limits or admitted charges.
Dedicated error-owner observability and full statement acceptance remain pending.

Fourteen exact mapped TX witnesses replayed 14/0 on committed `1b6c9d49`,
covering statement rollback, multi-collection atomicity, foreign-key races,
staged visibility, failed blocks, savepoints and session settings. Each named
command executed one passing test; these are current replays of existing
contracts, not additional pre-repair RED claims.

A focused combined-owner witness measured the portal fixture's materialized
source separately from cumulative portal retention. Data-only ownership held
512 bytes; the captured empty session overlay held 712 additional bytes, for
1224 combined bytes. The exact 64-row lower(payload) source with generated
LIMIT 1001/OFFSET 0 completed at the calibrated 258 KiB budget with a measured
execution peak of 262656 bytes. After dropping the result, 1224 remained until
controls and owner dropped, then zero. Owner references remained two before
and after the matching nested executor. The original denied request of 262656
bytes and this successful measured peak are distinct evidence. This does not
measure a pre-change baseline or the separate portal retention controls.
Independent numerical attribution review accepted the measured single-owner overlap and narrow fixture calibration. Full acceptance remains pending.

Six committed-generation source barriers passed: after capture, an independent
session commits a second row and warms the newer scalar, column-summary,
persisted fulltext, HNSW, hybrid or analytical-projection route. Captured
candidates, values and scores still equal the original oracle; the subsequent
statement equals the warmed newer oracle. These qualify existing behavior and
claim no new pre-repair regression. Path-specific diagnostics are retained
separately from the parallel suite output.

The whole-statement scan witness dispatched one scan with two workers over
1026 rows. It retained the captured committed rows and staged insert after an
independent commit and nested rollback; the next statement saw only the later
committed version. A direct materialized-source denial at the original budget
returned requested 262656 > 262144, with 244736 as the peak of successful
reservations before denial. Automatic-owner/row charge returned to zero; a
fresh admitted retry returned 64 rows and released to zero. These distinct
denied/successful-peak values are not interchangeable. The complete focused
statement suite passed 47/0 before the worker test-name convention correction.
Graph authority is qualified after the clean rebase onto merged #772 source;
full ordered validation and hosted acceptance remain pending.

Exact artifact diagnostics recorded four scalar range scans, four accelerated
column-summary scans, four fulltext posting retrievals, four HNSW executions
and hybrid ANN/posting retrieval. The analytical projection control retained
correct captured rows through its existing stale-or-unverified fallback; its
aggregate diagnostics include one optimized execution and four fallbacks, so
this does not independently establish an optimized captured projection read.
Jev identified insert-only old-candidate removal and changed-vector-score
qualification gaps at 0.73 and 0.65. Focused after-capture UPDATE controls are
required before final acceptance; no runtime defect is inferred from these
probabilities. They follow the clean rebase to current graph authority.

The clean rebase onto merged graph authority `b8c11e6f` completed without
conflicts. The post-rebase focused statement suite passed 52/0 after completing the time-series committed bucket barrier and diagnostic refinements. Three required
UPDATE barriers closed the insert-only qualification gaps: moving a scalar
value out of its indexed range and removing a fulltext token leave the captured
candidate present while the warmed and subsequent statements exclude it;
changing an indexed embedding preserves the captured distance while the later
statements use its new distance. The graph barrier changes an edge target and
weight after capture: the captured neighborhood remains bob/cost 1 and the
warmed and subsequent neighborhoods are carol/cost 9. An initial test-fixture
closure return-type compile error is preserved separately and is not a runtime
RED. Full ordered repository and hosted acceptance remain pending.

The committed time-series bucket barrier returned only amount 10 from the
captured view while warmed and subsequent statements returned amounts 10/20;
its four reads used native buckets with no fallback. Existing parallel scoring,
grouped aggregation and aggregation-timeout controls each replayed one passing
test on the rebased source. Exact non-interleaved UPDATE diagnostics recorded
four scalar range scans, two fulltext posting reads and four HNSW executions
with twelve ANN reads. Graph diagnostics recorded four neighborhood traversals
and 28 reads without fallback. Bounded Jev review returned 0.26 for an additional
finite local semantic gap; this does not establish full or hosted acceptance.
Owned test diagnostics now report fixed expectations or scalar failure flags
instead of complete SQL results; predicates and measured resource logs remain.

## Combined joined correlation and statement-view repair

The defined bundle is #761/#763/#864/#866. The composed preparation source
`cee93785` with the intersection witness committed as `06b56a18` established
RED: 0 passing, 1 failing, exit 101. A qualified joined outer alias was
unresolvable before the nested read could qualify the captured statement view.
The #866 literal-name checkpoint is composed at `28e345e5`; the selected repair
now preserves each joined leaf's existing alias and literal-field authority.
Static binding recurses joined leaves; runtime outer aliases survive projection,
EXISTS binding and scoped attachment with their original entry indices.
Qualified outer aliases remain authoritative; unqualified outer aliases are
admitted only when the inner relation does not define that column. New names,
alias vectors, lookup capacities and allocation overlap are admitted before
construction, with existing cancellation checks and type/query/operator leases.
The lazy SELECT callback, inherited statement Data owner/overlay, existing
2 KiB WHERE control and fresh command mutation authorities remain required.

The committed-barrier witness must return `[[1,true],[2,false]]` for the captured
statement and `[[1,false],[2,true]]` for the next statement, and assert the
independent writer committed after outer-left acquisition. Focused owner,
attachment, literal-field and finite relational/snapshot controls precede full
acceptance. This preparation does not claim acceptance: final repository checks
run only after rebasing onto the actual merged #798 predecessor, followed by
exact-head hosted checks and merged readback under the coordinator.

The first composed intersection repair passed its exact committed-barrier
witness (1/0). Independent source review requires explicit joined qualifier
length times field-count admission, including table replacement overlap;
generic source/schema clone estimates alone do not cover that cross-product.
The private static scope now reserves that bound before rendering names and
reserving the table. A threshold-minus-one denial and zero-charge cleanup
witness is required. Jev's right-alias index, inner-priority and joined literal
probabilities of 0.67, 0.67 and 0.45 require the three finite SQL probes.
Their results and final acceptance are pending.

The finite joined probes returned 4/1: right-side alias indices, inner-column
priority, the static cross-product denial/cleanup control and an existing
INSERT SELECT control pass. Ordinary JOIN SELECT of declared `"a.b"`/`"A.B"`
fields succeeds, while its correlated SELECT EXISTS fails on the exact quoted
qualified outer alias. This is an actual dynamic binding RED after static
classification succeeds. The selected private extension canonicalizes only
EXISTS outer aliases whose existing joined leaf and stored field establish
literal provenance, preserving the original aliases and entry indices too.
No shared source qualification or unsupported derived-label boundary changes.
Additional alias capacity and name/lookup overlap remain admitted first.

## Captured empty-overlay footprint

Combined qualification found actual RED in the unchanged 2 KiB uncorrelated
WHERE EXISTS control: requested 2269 bytes exceeds 2048. Its new joined scope
and outer-field admission are unused. The existing resource attribution witness
passes and measures Data 512, captured empty overlay 712, combined owner 1224,
materialized source peak 262656, owner references 2/2 and release to zero.
The empty overlay unnecessarily retains a whole map Arc and its conservative
empty-root allowance. Empty maps can retain allocated roots after removal, so
removing that allowance while retaining their map is not selected.

The private repair represents captured empty writes with `None` inside the
existing overlay owner, without retaining any map Arc. Nonempty COW maps keep
their current admission. Captured active-transaction state and weak session
identity remain retained and charged. A matching empty owner returns
`Some(false)` eligibility and `Some(empty)` staged snapshot, never live staged
state. This changes neither the statement-view contract nor public APIs.
The original 2 KiB SQL/budget/predicates stay unchanged. Actual footprint and
portal witnesses must precede restoring the historical 258 KiB portal fixture
to its original 256 KiB budget; historical calibration receipts remain evidence.
Nonempty/staged/savepoint/native controls and wire statement-view controls
remain required, with no global cap or accounting waiver.

The compact empty owner resource witness passes (1/0): Data 512 plus overlay
192 equals 704; source peak is 262136, retained 704 releases to zero, owner
references remain 2/2 and rows remain 64. Removing unretained map backing saves
520 bytes and fits the original 256 KiB cap with eight bytes to spare. The
unchanged old automatic-denial fixture actually succeeds at that cap; its
`expect_err` failure is preserved as an obsolete fixture boundary, not a runtime
regression. The positive 1024-byte-payload resource and portal fixtures now
return to the original 256 KiB budget. Only the denial fixture increases each
payload from 1024 to 1025 bytes to restore an actual denial at the unchanged
cap; error type, prior successful peak, zero release, retry rows and worker
assertions remain required. Actual denial requests 262328 bytes against 262144, with prior successful
peak 244344 and release to zero. The retry returns 64 rows and releases to
zero with no active operator workers. The portal lifecycle passes at its
original 256 KiB cap with all hold/deny/rollback/close/replacement assertions.

## Low-memory EXISTS fixture calibration

After compacting the empty overlay, the original 2048-byte control still fails
before EXISTS evaluates: outer Full conversion requests 2548 bytes. Exact
attribution is current 1620 = captured owner 704 + retained input 916, zero
identifier scratch, and complete output admission 928. The corresponding
owner-free overlap 1844 is a calculation, not baseline executable proof. This
path intentionally overlaps input/output backing, covered by converter denial
controls; projected-name scratch does not apply to Full rows with TEXT metadata.

The selected test-only calibration is 3 KiB: its original 2 KiB conversion
allowance plus 1 KiB for the measured captured owner. SQL, 64 inner documents
with 1024-byte payloads, one result row and exactly two visited scan entries
remain unchanged. A full-inner SELECT must produce ResourceLimit at the same
3 KiB cap and release current query memory to zero before the fresh scan
counter for EXISTS. This replaces the preparatory exact-2-KiB fixture constraint
while preserving pull termination under bounded memory. Production admission,
evaluation order and default caps remain unchanged. Original RED traces stay
preserved; revised GREEN and final post-predecessor full acceptance are pending.

The calibrated 3 KiB focused control passes (1/0), including full-inner
ResourceLimit, zero release, the original single EXISTS result and exactly two
visited entries. The unchanged 2 KiB RED remains historical evidence.

Preparation qualification passes: 56 statement-view controls, three wire
statement-visibility controls, the original 256 KiB portal lifecycle, the
44 finite relational controls and the added static-scope admission witness.
The binary build, workspace/all-target/all-feature pedantic Clippy, formatting,
seven touched-test validators and three repository policies pass locally.
These preparation checks do not replace final ordered full acceptance after
integration with actual predecessor main `ad0a001a65fae23726cd184fedf1f973d649e60c`.

## Statements without Data dependencies

Final integrated full qualification exposed four metadata-only SELECT failures:
version, pg_catalog.version, current_schema and current_database attempted Data
capture for a session naming the metadata-only database catalogdb and returned
NotFound. A focused nested SingleRow EXISTS statement reproduces the same RED.
The contract requires one captured owner before authoritative Data reads; a
statement with provably no Data dependency need not acquire that owner.

A focused private logical-plan predicate supplies the same conservative gate
at bound SELECT capture and both public command-free executor entrypoints. It
uses existing expression-child traversal, explicitly recursing EXISTS, source
alias/derived/JOIN branches, CTE definitions and set branches, and every query
expression position including window children and bounds. Actual collection,
CTE reference and table-function sources and unknown function paths continue
to require capture. Existing function-dependency inspection supplies builtin
knowledge; pg_catalog.version is additionally recognized from its existing
system dispatch. DML acquisition remains unconditional and no NotFound error
is swallowed. Nested real Data must still share the ancestor capture even
when its outer source is SingleRow. No public API, format, cap or evaluation
order changes. Full RED logs remain immutable; focused and fresh full
acceptance results are recorded separately.

The first new nested missing-database fixture advances past capture and then
reaches existing nested binding refusal for an absent catalog database. That
is preserved as an admitted-namespace boundary, not promoted to a binder
regression or expanded support. Nested SingleRow execution uses an existing
metadata database; a separate private gate witness checks absent storage
database, no statement owner and zero current/peak owner admission. The set
visibility fixture compares its unordered bag; no implicit row order is
selected. Existing scored/vector/index/rollup access paths require collection
sources, so their Data acquisition remains behind conservative source capture.

The no-Data repair passes the nine catalog probes, including all four original
full-run failures, and 59 statement-view controls. The zero-owner witness
records no retained statement read and zero current/peak admission; its owners
and session drop before explicit engine shutdown and strict directory cleanup.
The nested Data writer actually runs after ancestor capture for direct EXISTS,
derived, CTE and set branches, with captured true and next-version false; set
rows are compared as an unordered bag. The bound/window/function-child AST
dependency witness, three wire visibility controls, 34 relational controls,
three scoped-owner controls, four attachment controls and bounded WHERE
control also pass. Build, broad pedantic Clippy, formatting, three newly
touched-test validators and repository policies pass in preparation.

The private capture-helper extraction uses command-free final plans in the
admitted bound-execution path: exhaustive logical planner dispatch establishes
that these correspond to parsed SELECT, while Explain and transaction paths
return earlier and executable commands have explicit command plans. There is
no SELECT INTO AST variant. This preserves the original SELECT dependency
gate while keeping the bound-execution function under its line limit. Final
ordered full validation must restart on the new clean integrated head; the
prior partial full RED is preserved and is not reported as acceptance.

## Current integration review boundary

The refreshed 2026-10-09 integration retains the selected #763/#864/#866
implementation and the finite #761 qualification. Independent source review
found no confirmed statement-view defect. A new actual joined WHERE literal-field
RED (two failures, one inner-shadow control passing) selects reuse of the existing
admitted outer-row helper for JOIN sources only; ordinary and SELECT controls
succeeded before that WHERE failure. Its three focused regressions now pass.
The copy-owner/denial/cancellation probe passes. The normal-profile locked full
suite passes: 3,756 tests passed, zero failed, nine ignored. Full workspace/all-
target/all-feature pedantic Clippy, formatting, 43 touched Rust test-file checks
and repository test/document/benchmark/module policies pass. A subsequent
policy-required function rename has an exact body-equivalence receipt and a
passing focused probe. Hosted CodeQL then flagged eleven test-output locations;
static triage found fixed fixture/test-only paths rather than a production
credential flow. Unnecessary diagnostics are removed, and unordered truth/carrier
matching consumes a Boolean slot per occurrence without removing payload rows.
Literal expectations, multiplicity, descriptors and runtime code are unchanged.
Focused ownership/relational and broad quality rechecks are recorded separately
from the full run. Final hosted checks and merge evidence belong to the PR record.

The finite sibling diagnostic records successful uncorrelated HAVING, ORDER BY,
JOIN ON and window controls, with correlated failures tracked by
[bug #868](https://github.com/cntryl/cassie/issues/868). Grouped SELECT correlation
returns its exact expected rows. UPDATE RETURNING leaves both EXISTS variants
unresolved, tracked by [bug #869](https://github.com/cntryl/cassie/issues/869).
The recording harness's exit zero is not desired-behavior acceptance. These
findings keep #761 open; this bundle does not claim sibling or RETURNING repair.

The [delivery tracks](query-engine-delivery-tracks.md) retain primary ownership,
activation gates and serial integration. #861 remains a separate unresolved
wait-attribution investigation; its already-merged finite teardown repair is
not proof of the multi-minute wait's cause.

## Nested ancestor scope repair (#870)

The accepted baseline `ce72a23f2d0a3cdb9a811107aec7629151fcd319`
reproduced the nested SELECT and WHERE ancestor binding failure tracked by
[#870](https://github.com/cntryl/cassie/issues/870). A first-level positive control
passed. The focused repair retains the existing admitted ancestor row chain and
supplies its qualified field authority during nested binding. A borrowed source
namespace walk detects enclosing references inside deeper EXISTS before eager
folding; inner names retain precedence. Ordinary uncorrelated occurrences keep
the existing eager path, and correlated CASE/COALESCE uses selected evaluation.

Focused relational witnesses cover exact nested Boolean/WHERE rows, distinct
middle and ancestor values, selected CTE/derived intermediates, same-alias inner
shadowing, quoted `a.b`/`A.B` identity, and dead/reached scalar branches. Tests live
in `tests/relational_qualification/ancestor_scope.rs`. The original RED and later
mixed-scope RED are separate from passing controls. This finite repair does not
qualify arbitrary nested grammar or complete SQL-022 correlation.

The committed writer barrier checks nested SELECT and WHERE against the
captured membership, then verifies a fresh next statement sees the update. A
separate staged-overlay witness rolls back the live session after capture and
retains its staged membership only in the captured nested statement. Ancestor
copy and inspection witnesses measure admitted peak, deny peak-minus-one,
check cancellation cleanup, preserve typed parent owners through the final
reader, and require zero charge on retirement. Focused controls pass; full
local and exact-head hosted validation,
independent review, merge and issue readback belong to the PR record. These
finite witnesses do not establish native release or production readiness.
#761 remains open while #868 and #869 remain unresolved. Support and production
readiness remain unchanged.

The integration full run exposed outdated fixture boundaries. Tests that exercise
first-wide-row denial and bounded identity scans now admit a finite 4 KiB budget:
the captured owner charges 704 bytes before reading, while each wide payload is
8 KiB. Exact first-entry counts, poisoned later rows, one/four-worker coverage and
zero final reservations remain required. The positive two-row overlay/sort fixture
uses 32 KiB; this does not raise runtime limits or replace separate denial probes.
ANN mutation barriers compare the paused reader against an exact pre-index oracle
and distinguish it from a fresh post-mutation statement. Plan-cache database
isolation creates two real Data families and checks distinguishable literal rows
and cache-counter deltas. These fixture changes pass the final full local acceptance run.

## Deferred EXISTS in late expression phases (#868)

The proposed repair for [#868](https://github.com/cntryl/cassie/issues/868) carries
the statement's captured Data view and immutable staged overlay through HAVING,
scalar ORDER BY, JOIN ON residuals, and window arguments. These phases use the
existing statement environment; they do not acquire a newer Data view. HAVING
uses the actual post-aggregate group row. ORDER BY evaluates each key from its
actual input row before sorting; comparisons consume retained semantic keys.
JOIN evaluates the combined candidate row before outer-join null extension.
Window arguments use the input row selected by the window frame.

The common borrowed resolver preserves qualified field authority, structured
column types, and the current and ancestor row owners. Existing query and
operator reservations remain alive while a deferred expression reads the row;
additional owned carriers are admitted before copying. Cancellation and deadline
checks retain the ordinary phase behavior. Sort key retention has its own
admission and does not substitute for the original row's ownership.

The deterministic [phase visibility witnesses](../src/app/statement_read_tests/phase_visibility.rs)
commit membership from ID 1 to ID 2 through an independent writer after the outer
source has been acquired and before the late phase reads it. The captured and
fresh statements must produce, respectively: HAVING rows `(1, 1)` and `(2, 1)`;
ORDER BY IDs `[2, 1]` and `[1, 2]`; JOIN ON IDs `[1]` and `[2]`; and window argument
values `[(1, true), (2, false)]` and `[(1, false), (2, true)]`. The writer barrier
propagates errors and the fixture requires zero final query charge and workers.

The [carrier admission witnesses](../src/executor/execution/exists_phase_tests.rs)
check exact structured payload type, original input charge, ancestor owner
liveness before and after evaluation, and retirement after the final reader.
They measure an admitted peak, reject peak-minus-one, and check cancellation
without prematurely retiring input owners. Selected SQL behavior is qualified
by [HAVING](../tests/relational_qualification/exists_having.rs),
[ORDER BY](../tests/relational_qualification/exists_order.rs),
[JOIN ON](../tests/relational_qualification/join_on.rs), and
[window argument](../tests/relational_qualification/exists_window.rs) witnesses.

All six focused phase owner/visibility probes pass, including all four captured
writer barriers. The independent sort invocation probe also passes, retaining
the input owner through output consumption. Complete ordered local validation,
exact-head hosted checks, independent review, merge and issue readback belong to
the PR record. These selected phases do not qualify EXISTS in window
partition or window ordering expressions, generic set-output scope, or arbitrary
correlated grammar. #869's RETURNING acquisition choice remains unresolved and
is not specified here. Support, native release, and production readiness remain
unchanged.
