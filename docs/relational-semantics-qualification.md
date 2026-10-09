# Relational semantics qualification

This finite qualification slice tracks [#761](https://github.com/cntryl/cassie/issues/761)
and the confirmed correlated SELECT output defect
[#864](https://github.com/cntryl/cassie/issues/864). Qualification is in progress;
the selected public grammar, type identities and support levels remain the
contracts in [the SQL dialect profile](sql-dialect-profile.md).

## Selected correlated output repair

Classify each `EXISTS` occurrence independently using the existing binder's
inner/outer scope authority. A successful bind without outer fields remains
uncorrelated and is folded before source execution. When that bind fails but
binding the same statement against the admitted enclosing source fields
succeeds, only that occurrence is deferred to the actual outer row. If both
binds fail, preserve the original error. This classification performs no
subquery execution and uses the same CTE scope, parameters and session context.

Uncorrelated siblings still fold before source execution, including when the
outer input is empty or another sibling is correlated. Deferred SELECT output
occurrences resolve per actual row after filtering, grouping, windows and
ordering, before projection, DISTINCT and final pagination. This remains a
scalar expression boundary and does not promote aliases or correlation to a
native kernel. Fresh AST, field, lookup and row copies must be admitted before
allocation and retain the existing parent/query/operator owners through their
handoff. Cancellation and errors release temporary owners. The source attachment
copy has its own pre-allocation lease even while the original outer row remains
live. Deferred correlated output uses the existing scalar evaluator's lazy CASE
and COALESCE selection through a private borrowed resolver; only reached nodes
execute, and conditions/arguments are evaluated once. Static uncorrelated
folding, including dead-branch and empty-input errors, remains unchanged.

Qualified references reserve their actual parsed namespaces during private
carrier allocation, including nested scopes. Alias lowering preserves an
inner carrier only when the enclosing admitted namespace would collide with
the underlying collection qualifier. An exact binder-supplied outer field
may pass alias lowering only when no inner namespace claims its qualifier.

The slice preserves existing uncorrelated and WHERE behavior. CASE, supported
function wrappers, inner shadowing, CTE scope, empty input, grouped output,
errors and owner retirement require concrete controls. HAVING, ORDER BY and
join ON behavior is not silently moved to a different evaluation phase; any
unsupported sibling discovered by qualification receives an explicit finding.

## Evidence status

The 14 seed tables have passed actual PostgreSQL-wire parameter OID and stored
carrier readback, including NULL arrays, negative zero and integers above
2^53. Literal query rows and descriptors are separate from implementation
helpers. All 22 primary literal cases and their Statement Describe/text and
binary Portal descriptors now pass focused qualification. The confirmed
three-shape correlated SELECT defect has actual red/green evidence, with
WHERE, inner shadow, CTE, CASE/function and eager empty-input/sibling controls.
The original failed attempts and preparation corrections remain retained;
this is not a completed qualification or a full-suite pass. Independent copy
ownership, threshold-minus-one denial, cancellation and final-reader retirement
now pass focused controls, including warmed lookups with spare vector capacity.
Selected CASE/COALESCE dead branches, reached errors, dynamic Boolean parameters
and a session-local volatile CASE operand pass; scalar callbacks run only for
reached nodes and preserve the independently parsed function-body error boundary.
Developer quality and a coherent implementation checkpoint remain separate from
completion of the remaining qualification matrix and full validation.

Completion still requires all 22 primary invariants, concrete positive and
negative variants, pagination parameters, rich keys, frames, operator path
witnesses, selected PostgreSQL 18 oracle records, and normal required local
and exact-head hosted validation.

## SQL-006 preparation correction

The positive variant originally assumed a positional alias list on a derived
subquery, `FROM (SELECT n,a FROM r WHERE id=3) AS q(x)`. The current subquery
parser treats the whole suffix as its alias; actual execution rejects `q.x`
with SQLSTATE 42601. D1 selected prefix lists for ordinary Collection/Cte aliases,
so this is an unsupported preparation input, not a regression or a request to
extend the grammar. Preserve that failed attempt. The corrected variant uses
`WITH c AS (SELECT n,a FROM r WHERE id=3) SELECT q.x,q.a FROM c AS q(x)`, with the
same independent NULL rows and exact descriptors. The primary derived-source
case keeps its supported inner projection alias `n AS x` and ordinary `AS q`.

The corrected CTE prefix variant preserves output labels `x` and `a`: binder
`aliases::lower_select` sets the projected alias from `ColumnIdentifierPath::declared_name`;
`projection::compile_projection_ops` uses that explicit alias. The original
prepared `q.x`/`q.a` metadata described the unchanged derived primary, not this
CTE variant. All twenty variants executed successfully before this metadata
assertion failed; the failed descriptor attempt remains retained.

## Finite scalar frame qualification

The independent frame records use four ordered BIGINT rows `(1,10)`,
`(2,20)`, `(3,20)`, `(4,30)`. Peer groups are `[10]`, `[20,20]`,
and `[30]`; endpoint expectations for GROUPS offsets and all three EXCLUDE
forms are materialized before execution. Both statements retain the selected
scalar frame path; this adds no syntax or native-path promotion. Embedded rows
and text/binary Statement/Portal descriptors must match those literals.

## Negative record scope

The alias-arity negative is `WITH c AS (SELECT id FROM r) SELECT q.x FROM c AS q(x,y)`: one source field and two prefix aliases must reject under the selected CTE law. The earlier derived-list draft is unsupported preparation, preserved separately. Ordinal bounds, incompatible CASE outputs, recursive arity/types, and D2 bound errors retain their existing parser/binder/wire authorities; no rejection is treated as a request to add syntax.

## Completed-path witness scope

Scoped caller-thread subscribers retain every relational completion event, not
a single last-path slot. Direct primitive sort/top-k/distinct/set/distinct-on
and ranking windows must report their selected admitted paths. Expression
ordering, CTE DISTINCT, and GROUPS frames must retain their named scalar
boundaries. A composed DISTINCT then sort query requires both events. These
witnesses do not infer worker coverage or private input handoff from an event;
those are separate controls. No global subscriber is installed.

## D2 repeated Bind qualification

For one prepared query per text/binary format, rebind limits/offsets `2/1`,
`1/2`, `0/0`, `NULL/0`, and `2/NULL` without reparsing the statement between
binds. The fixed ordered eligible ids are `[5,1,2,6]`; exact results are
`[1,2]`, `[2]`, `[]`, `[5,1,2,6]`, and `[5,1]`. Initial parameter Describe
advertises BIGINT/BIGINT, Statement Describe is text, and every Portal Describe
and DataRow preserves its Bind format. No stale bound or portal result may leak.

## Focused evidence-gap probes

Jev bounded review scored the preparation-oracle question 0.36 and scoped-event
attribution question 0.39. The selected probes verify quoted CTE prefix labels
`X`/`a` and BIGINT/ARRAY descriptors on NULL-only and empty inputs, and two
concurrent independent caller subscribers with a barrier: primitive DISTINCT
versus scalar GROUPS window. Neither subscriber may record the other owner.
These probes do not assert async worker coverage or whole-matrix acceptance.

## Private owner-partition qualification

DISTINCT consumes real batches. A 1,025-row declared BIGINT fixture repeats
`NULL,1,2,3,0` classes and splits at 0, 1, 512, 1,023, 1,024, and 1,025.
Literal first occurrences are `[NULL,1,2,3,0]`; each nonempty input partition
owns a separate 256-byte source lease marker. Winners retain their actual source
markers until output drop, and every marker and operator charge then retires.

Set kernels consume complete row vectors. The existing adapter boundary receives
partition-owned rows before flattening; no streaming set API is selected.
The 4x5x3 source-partition grid for `sa UNION (sb INTERSECT sc)` preserves the
independent bag `[NULL,1,2]`, with `sa=[1,1,NULL]`, `sb=[1,2,NULL,NULL]`,
and `sc=[2,NULL]`. The four two-branch operators separately preserve their
recorded bags. Input partitions include empty endpoints, declared types and
source lease markers; output ownership and final zero charge are observed.
Marker reservations witness existing source-owner retention, not an estimate of
all test-fixture allocation. No runtime operator or public API is added.

## Finite PostgreSQL and homogeneous join qualification

Installed PostgreSQL 18.6 runs in an owned UUID cluster, UTF8/C-libc/UTC,
with a private loopback endpoint. Forty-five preselected row comparisons passed:
19 compatible primary cases, 20 variants, two homogeneous BIGINT join controls,
two frames and two quoted-prefix NULL/empty cases. Unsupported Cassie SQL-002
and original mixed exact-number join cases remain excluded; ARRAY OID and
SUM/AVG descriptor parity are not inferred. Binary-derived FLOAT seed negative
zero remains exactly `8000000000000000`. The first metadata-only attempt failed
before any case because `lc_collate` is database catalog metadata, not a PG18
SHOW parameter; that failed attempt and both strict cleanup receipts are retained.

The homogeneous Cassie control uses independently enumerated seven LEFT and
five INNER pairs with vectorized joins disabled and enabled. Actual unaliased
LEFT joins report `vectorized`; INNER joins report `typed_hash`. Both report
build4/probe5/matched5; disabled controls report merge with zero vectorized build.
The legacy semantic-key LEFT diagnostic reports four non-NULL retained build
rows as `right_input_rows_total`; typed INNER and merge report all five right
source rows. Ordinary aliases preserve the same literal rows and `id`/`id`
BIGINT descriptors in both wire formats. These are case-specific actual path
witnesses, not native eligibility inferred from a configuration flag.

Initial prepared metadata, blanket native/fallback attribution and physical
right-input counter assumptions failed against current source. Those attempts
remain preserved separately from runtime red/green evidence. The LEFT typed
eligibility checks actual right-row/template entry and alias agreement before
conversion; declining that check retains the existing semantic-key path. No
runtime kernel, metadata rule, metrics implementation or public API changes.

## Final selected split controls

The next finite controls preserve the existing complete-input join adapter and
window batch entrypoints. Homogeneous five-row sides receive all 6x6 source
partition boundaries before their admitted flattening; independent LEFT/INNER
pairs and actual typed completion remain required. The original six-row window
seed receives splits 0..6, singleton batches and `[2,1,2,1]` chunks, with literal
rank/dense-rank outputs and declared types. Separate source markers witness
retention and final retirement; no streaming join API or shared scalar primary
oracle is introduced. Existing typed join NULL/output-budget controls, rich
DISTINCT fallback and recursive depth/resource controls supply their precise
selected ownership checks when actually replayed on the refreshed source.

The same final snapshot adds ordered private mixed-numeric DISTINCT literals
(raw negative-zero first winner, exact large integer versus rounded float) and
declared ARRAY NULL/empty/element-NULL identity across every batch split. These
retain the existing semantic-key fallback and source owner until output drop.
The review's quoted literal-dot column candidate first checks ordinary SELECT
and WHERE EXISTS before correlated SELECT, using identical two-row inputs;
unsupported baseline syntax is not treated as an invitation to extend grammar.

## Selected delimited-field correlation repair

[Bug #866](https://github.com/cntryl/cassie/issues/866) records actual ordinary
quoted-field SELECT success with matching WHERE/SELECT EXISTS scope errors.
A literal stored component such as `a.b` must be distinguished from an encoded
qualified path using current source/schema/alias provenance and existing
`ColumnIdentifierPath` authority, preserving the original WHERE phase and
per-occurrence SELECT folding. Collection fields, selected CTE prefix renames
and derived projection labels are probed separately before choosing the private
naming seam. No punctuation-wide rewrite, new grammar, public API or per-row
schema clone is selected. The original unsupported derived positional alias-list
boundary remains unchanged. All current owner/cancellation and 2 KiB WHERE
controls remain required.

The finite #866 seam selects exact declared Collection fields, explicit derived
projection aliases, unqualified ColumnIdentifierPath output components, and
wildcard fields inherited from a single declared Collection (through ordinary
aliases). Successful CTE-prefix remapping and supported derived `q.id` paths
remain unchanged. The attempted outer `q."r.id"` reference to an unaliased
qualified derived projection is unsupported even in ordinary SELECT and is
recorded as a preparation error, not a grammar expansion. Literal raw names are
escaped through the existing identifier helper before the current qualifier is
prepended in static SELECT scope, dynamic SELECT rows and original WHERE rows.

Current focused #866 regressions retain ordinary, WHERE and SELECT results for
quoted dotted fields, escaped quotes, exact uppercase fields, successful CTE
prefix remapping and the selected derived source shapes. The final literal
inner-priority probe yields `true,false`; distinct `a.b`/`A.B` fields yield
`false,true` in each row, with BOOLEAN descriptors. These answer the concrete
Jev 0.74/0.64 candidates through actual tests, rather than model approval.
The 34-test relational suite and unchanged 2 KiB WHERE control pass locally;
three scoped-copy and four attachment admission/cancellation controls retain
their owners and reject the observed threshold-minus-one budgets. The private
join/window partition controls also pass. This is a focused checkpoint:
combined joined-source/writer-barrier qualification, required full validation
and exact-head hosted acceptance remain pending before issue closeout.

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

## Selected correlated evaluation phases (#868)

The next finite repair addresses the four supported contexts recorded in
[#868](https://github.com/cntryl/cassie/issues/868), after the accepted ancestor
scope repair in `700cbad7bc87801b51eceb0b532451d0dc021d1b`. It reuses the existing
per-occurrence classifier and an admitted actual-row carrier, with a borrowed
scalar resolver whose lifetime ends with that evaluation. The enclosing
statement's CTE context, captured data view, controls, type identity, aliases
and ancestor ownership remain authoritative. Independent occurrences retain
existing eager folding; deferred occurrences follow reached CASE/COALESCE
branches rather than resolving every branch before scalar evaluation.

The selected phase authorities are:

- HAVING reads the actual grouped row after aggregate-expression rewriting.
  The finite controls distinguish multiple group keys, group cardinality,
  selected scalar branches and empty grouped input.
- ORDER BY computes correlated semantic keys from each actual row or group
  before comparisons. Existing alias and grouped-expression rewriting must
  precede deferred detection and key evaluation. Comparisons do not execute
  subqueries. The selected controls cover deterministic Boolean ordering,
  tie keys, LIMIT and a projection-alias probe.
- JOIN ON evaluates the actual combined candidate before recording a match or
  emitting an outer-join null extension. The finite controls distinguish
  rejected candidates from post-join filtering, exact alias identity, selected
  lateral/ancestor inputs and scalar branch selection. Existing pure-equality
  routes retain their qualification; a correlated residual must be evaluated
  or use an explicit supported scalar route.
- Window value arguments read the frame-selected input row. Default and full
  LAST_VALUE frames plus LAG distinguish that row from the current output row.
  Existing typed eligibility, frame selection and ordinary argument evaluation
  retain their authority.

Correlated window PARTITION BY and window ORDER BY expressions are outside this
selected argument repair. Set-output ordering scope is also outside this finite
profile. These boundaries do not promote unsupported grammar, establish
universal correlation, or qualify SQL-022 as complete. RETURNING remains the
separate [#869](https://github.com/cntryl/cassie/issues/869) evaluation-phase
contract; no DML visibility or atomicity rule is inferred here.

The accepted-main HAVING reproduction is preserved separately from the earlier
sibling diagnostic's recording-only exit zero. Focused desired-behavior tests
are grouped in `tests/relational_qualification/exists_having.rs`,
`exists_order.rs`, `join_on.rs` and `exists_window.rs`. Common admission and
captured-statement witnesses check retained input owners, measured peak-minus-one
denial, cancellation retirement, committed writer barriers and fresh-statement
discrimination. The focused relational suite passes all 57 tests, including the
projection-alias ordering probe, DISTINCT ON representatives and the selected
phase cases. The separate six phase owner/visibility probes and one exact
sort-key invocation probe pass. Existing typed sort/window controls pass 10/10
and 7/7. Normal ordered full validation, exact-head hosted checks, independent
review, merge and issue readback belong to the PR record. Historical RED receipts
remain unaltered. #761 stays open while its selected blockers remain unresolved;
support and production readiness are unchanged.
