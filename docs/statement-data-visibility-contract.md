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
