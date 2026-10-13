# PostgreSQL 18 Delivery

This is the top-priority, dependency-ordered delivery record for
[#791](https://github.com/cntryl/cassie/issues/791) and the
[Postgres SQL feature parity milestone](https://github.com/cntryl/cassie/milestone/6).
The [selected dialect profile](sql-dialect-profile.md) defines full PostgreSQL
18.6 core SQL and PL/pgSQL scope and the only architectural exceptions.
The baseline is `769d4db8fb7e60f0a29fade73137433a20e5a817` (2026-10-10).
The initial inventory was 57 open and 12 closed; thirteen new owners #904–#916
extend it to 70 open and 12 closed before subsequent work. These are timestamped
planning counts, not completion evidence; refresh GitHub before later claims.

## Delivery stages

| Stage | Owners and order | Outcome |
| --- | --- | --- |
| 0 | #791–#793; #904–#916 | Scope, ownership, dependencies and bootstrap inventory/harness; retain #848 final gate. |
| 1 | #869 → #761; #768/#769; #819/#833; #904 | RETURNING/relational acceptance, codec/mutation qualification, isolation, transactional catalogs, shared type runtime. |
| 2 | #796, #798–#801, #803, #812–#816, #922/#923; #762 → #777 → #780 | Grammar/expressions/scalars; row grammar before row-subquery acceptance; catalog/client/wire foundations. |
| 3 | #804–#811; #817/#818; #820–#822; #905–#908 | Collections, aggregation, sessions, ranges/XML/geometry/money. |
| 4 | #823–#828, #834, #838–#841, #845/#846; #909 | Mutations/schema; constraints → table foundation → composite/domain integration. |
| 5 | #829–#832, #835–#837, #842–#844; #910–#916 | Full PL/pgSQL and remaining SQL objects, rules, statistics, local two-phase transactions and large objects. |
| 6 | #847/#848; #779 and prerequisites; #8 → #29 → #781; #849 | Complete client/dialect/cross-path/operational/restart acceptance on immutable revisions. |

Stages are grouping, not permission to bypass issue dependencies. #829 is
grouped with stage 5 programmability even though its JSON prerequisites can
prepare earlier. External prerequisites retain their existing milestones and
share this program's priority. Preserve completed foundations and finish only
their remaining acceptance obligations.

## New focused owners

| Owner | Outcome | Implementation prerequisites |
| --- | --- | --- |
| [#904](https://github.com/cntryl/cassie/issues/904) | Implement the shared PostgreSQL type and catalog runtime | #792, #793; extract oversized catalog modules before extending them |
| [#905](https://github.com/cntryl/cassie/issues/905) | Add PostgreSQL range and multirange types | #793, #904; #812 for numrange and #814 for tstzrange signatures only |
| [#906](https://github.com/cntryl/cassie/issues/906) | Add PostgreSQL XML values and SQL/XML functions | #793, #904 |
| [#907](https://github.com/cntryl/cassie/issues/907) | Add PostgreSQL geometric types and functions | #793, #904 |
| [#908](https://github.com/cntryl/cassie/issues/908) | Add PostgreSQL money values and arithmetic | #793, #904, #812 for numeric conversion signatures |
| [#909](https://github.com/cntryl/cassie/issues/909) | Implement PostgreSQL table inheritance | #826, #824, #833, #828 for named row-type integration |
| [#910](https://github.com/cntryl/cassie/issues/910) | Implement SQL-defined aggregate objects | #830, #804, #793, #904, #833 |
| [#911](https://github.com/cntryl/cassie/issues/911) | Implement SQL-defined casts and conversions | #830, #793, #904, #833 |
| [#912](https://github.com/cntryl/cassie/issues/912) | Implement SQL-defined operators and operator families | #830, #793, #904, #833 |
| [#913](https://github.com/cntryl/cassie/issues/913) | Implement PostgreSQL SQL rewrite rules | #833, #839, #823 for mutation actions |
| [#914](https://github.com/cntryl/cassie/issues/914) | Implement PostgreSQL extended statistics | #833, #842 ANALYZE implementation, #817 catalogs |
| [#915](https://github.com/cntryl/cassie/issues/915) | Implement local two-phase transactions and recovery | #819, #833, #793 durable format record, #820 locking |
| [#916](https://github.com/cntryl/cassie/issues/916) | Implement Midge-backed PostgreSQL large objects | #793 durable format record, #904, #819, #833, #831 privileges |
| [#922](https://github.com/cntryl/cassie/issues/922) | Match PostgreSQL planning-time constant expression errors | Shared expression/type/coercion and function-volatility contracts; keep runtime laziness distinct |
| [#923](https://github.com/cntryl/cassie/issues/923) | Avoid evaluating irrelevant EXISTS target expressions | Shared expression/type planning; pin target-elision contexts against PostgreSQL before implementation |

The two expression-planning owners were discovered by actual PostgreSQL 18.6
comparison while qualifying #869. They remain stage-2 runtime work and final
integration obligations for #761/#848; they do not invalidate the finite
RETURNING callback repair or become architectural exclusions. Its runtime
witnesses use row-dependent errors and PostgreSQL-qualified constraint ordering.
The next #904 [mixed numeric SQL comparison contract](mixed-numeric-comparison-contract.md)
reconciles the remaining scalar/join operator-coercion blocker before #761's
complete relational acceptance. #869's merged finite repair remains accepted;
query comparison qualification does not complete #769 CON-05 or full #798.

## Existing-owner extensions and staged dependencies

| Owner | Additional acceptance |
| --- | --- |
| #800 | TABLESAMPLE SYSTEM/BERNOULLI/REPEATABLE, explicit randomness assertions; complete row grammar before #762 row-subquery acceptance. |
| #816 | pg_lsn SQL value, OID/codecs/operators/functions; replication and physical PostgreSQL WAL administration remain excluded. |
| #824 | Exclusion constraints, PostgreSQL 18 temporal constraints, concurrent conflict enforcement; consume #905 built-in primitives. |
| #828 | Composite/record/domain only; range/multirange scope and all associated operators/functions/aggregates move to #905. |
| #837 | Event triggers in addition to row/statement/constraint/INSTEAD OF trigger execution. |
| #842 | Real Midge-backed VACUUM/ANALYZE/CHECKPOINT/REINDEX/CLUSTER effects, index/operator-family integration; accepting syntax alone cannot complete maintenance. |
| #835 | Full PL/pgSQL; the previous procedural subset is intermediate. |
| #793/#812/#807/#814 | Complete type records and final PostgreSQL numeric, array and temporal semantics; earlier finite limits are intermediate. |

Built-in ranges #905 depend on the #793 records and #904 registry, with #812
required only for numrange and #814 only for tstzrange signatures. They do not
depend on #824/#826/#828. Constraints then consume completed range primitives;
#826 follows constraints; #828 follows the table foundation. Custom-range
support functions and range/index integration are staged obligations under
#830/#912/#842, not blockers on unrelated built-in primitives. This breaks the
former range → constraint → table → composite cycle.

The [#925 structural prerequisite](https://github.com/cntryl/cassie/issues/925)
extracts projection virtual-catalog descriptors and row builders into the
private `src/catalog/virtual_views_projections.rs` module before #904 extends
the catalog runtime. Public entrypoints and database visibility stay in
`src/catalog/virtual_views.rs`. This extraction preserves existing behavior;
it does not implement the shared registry or qualify new SQL types, catalog
publication or durable formats. Record its merged commit and existing catalog,
database-scope and projection witnesses before consuming it as completed.

That prerequisite is accepted as [PR #926](https://github.com/cntryl/cassie/pull/926),
commit `2e7040dbce1f2235ba62ef2fbca52bee327a253c`: full ordered gate 3,845 Rust
passes, eight focused catalog/visibility/projection/wire witnesses and all 15
fresh hosted checks; exact merged tree verified, #925 closed. The subsequently
accepted [#928 bundle](https://github.com/cntryl/cassie/issues/928) centralizes metadata for
existing scalars and qualifies PostgreSQL Boolean binary reception. It changes
no public Rust signatures, legacy durable identities or mutable catalog roots;
#819/#833 remain open without being consumed by immutable builtin lookup.
Full #904/#793/#791 acceptance, named uint32 wire identities, all coercions,
new types and actual v3 upgrade round trips remain open. The independent native
ARM canonical failure stays failed under #927; its external remedy does not
become a circular prerequisite on unrelated static scalar primitives.

Record an implementation prerequisite as `owner; completed commit; exact tests;
passed result; remaining integration obligations` before dependent work starts.
A closed issue without that evidence is insufficient. #793 specifies contracts;
it does not depend on every type being implemented. Bootstrap #848 begins now;
its final gate still waits on every owner, client and operational obligation.

## Bundle and closeout policy

The private parameter-analysis extraction is accepted as
[PR #932](https://github.com/cntryl/cassie/pull/932), commit
`fbcc118ae63b02a35ea7d052e7333281820161ab`, tree
`ca4d20dee587321070c92c1eb21d323ee86efda7`: the full ordered local gate
passed 3,861 tests with nine existing ignores, plus 34 exact regression
witnesses, and independent final review found no material findings. Required
GitHub rules and the merged tree were verified; optional checks were pending
at merge. This is a structural prerequisite, not parameter semantic parity.

The finite #904 prerequisite accepted in [PR #933](https://github.com/cntryl/cassie/pull/933)
admits transient parameter metadata for nonempty wire Parse against fresh shared request controls, after authorization and
before normalization, inference or prepared-statement publication. A checked
borrowed AST traversal and fallible allocations reuse the existing inference
visitor. Allocation failures use a private Parse error route (`53200`), logical
parameter bounds use `42P02`, and cancellation/deadline checks use `57014`.
Admitted inference and legacy conflict errors retain their existing behavior.
The recorded safe RED uses 4,096 known INT4 parameters under a 1,024-byte
budget: the original runtime returns ParseComplete instead of `53200`.
Recovery must ignore Describe until Sync and permit the same statement name
to be parsed and closed. Separate tests on the same controls must prove
transient reservations release and their capacity can be reused.

The logical limit comes from the pinned PostgreSQL 18.6 source archive,
SHA256 `555610c24d53e4316da5b7d3fc25c279d96856d5e0e23ee308c328c5fa881d9f`:

| Source | Contract | Member SHA256 |
| --- | --- | --- |
| `src/include/utils/memutils.h:40` | MaxAllocSize = 1,073,741,823 | `cd136ef822c024b97374a659366d0f9fb7270324ef67597b323b34f8d1706b47` |
| `src/include/postgres_ext.h:32` | Oid is the selected four-byte unsigned identity | `7f2d7945243abf649e0c574457db63abd3b704603db245e15f2268e823154322` |
| `src/backend/parser/parse_param.c:141` | One-based parameter number is 1 through 268,435,455 | `4aab2ecc770fcda619b26ac12beaf7e527542b56f55eb45879b72e92442e8a28` |
| `src/backend/utils/errcodes.txt:375` | Undefined parameter is `42P02` | `6e8de346643ba84aa3c9c6a73360acfc7b2dfb89162c06c08ce9bf5bcd5bbcba` |

Checked arithmetic and the zero-based maximum 268,435,454 precede slot
allocation. Constructed AST indices such as `usize::MAX` must reject safely;
they do not imply a native PostgreSQL message or position. This bundle does
not qualify recursive inference interruption, registered client CancelRequest
during Parse, retained prepared/cloned vectors, empty-query allocations or
typed Execute/Describe admission. Those obligations and the confirmed
occurrence/coercion gaps remain open under #904/#849 and the full owners.

The #904 function-input foundation is accepted in [PR #935](https://github.com/cntryl/cassie/pull/935),
commit `3304f103e6484102b3c60588469009ee240baa7e`, tree
`75c3c7105e5f226b03d4a657667527c05608887a`: the complete ordered gate
passed 3,963 tests with nine existing ignores. Its ten function-input groups use an actual
immutable PostgreSQL 18.6 baseline: three lower/nested-COALESCE forms, unknown
COALESCE, BOOLEAN/INT/BIGINT typed NULL anchors, a Boolean COALESCE predicate,
and same-name TEXT/BOOLEAN-input SQL functions in two session schemas. Each
group covers OID 0/705/matching declared identity, non-NULL/NULL and text/binary
results, for 120 paths. Four wire tests and four public inference controls
capture the selected contract. Lower input rules use existing FunctionIds;
COALESCE shares the binder's common-type resolver. Scoped catalog input records
are borrowed under interruptible read guards with admitted lexical/path scratch;
function bodies and alias maps are not copied into the new Parse path.

The original baseline at merged `5099e47c86f93e74f9726d0192115341091ad44e`
retains all 134 paths/536 side observations: 80 function parameter-identity gaps
(16 also fail Execute), 40 agreeing known-input controls, and 14 separate
known-Boolean observations. The selected implementation does not close the
eight known-Boolean error/phase gaps or two unaliased descriptor-name gaps.
Invalid overloads/arity, other callable families, a generic UNKNOWN finalizer,
retained prepared memory and the original occurrence gaps remain open. Public
Vec/Option signatures, AST/catalog layouts and persistence are unchanged;
this finite slice cannot close #904/#793/#791/#848/#849.
The accepted post-fix observations agree on all 120 selected paths plus 36
scoped/nested-function paths, in both raw protocol and libpq adapters. At that merged foundation, the 14
known-Boolean controls retain ten differences and four matches. Post-SET
`search_path` ParameterStatus remains a separately observed difference under
#780 with #821 integration; successful function results do not qualify it.

The finite occurrence increment is defined in
[#904's selected contract](https://github.com/cntryl/cassie/issues/904#issuecomment-6115604094).
Its qualification covers the selected primitive SELECT contexts, including
existing-path controls, protocol recovery, interruption and memory ownership.
The PR acceptance record binds those checks to the actual tested revision;
complete shared-registry acceptance remains separate.

One private fold records each parameter occurrence's creation identity and its
own coerced identity, separately from the shared parameter slot. It analyzes
covered CTE/FROM sources before targets, transforms and coerces each Boolean
argument before its sibling, visits OFFSET before LIMIT, and only then finishes
eligible direct UNKNOWN target parameters as TEXT. Known input identities
survive explicit casts with different output identities. An earlier unresolved
IS NULL child remains an occurrence to check; it is not itself a TEXT target.
Selected inconsistencies use structured private `42P08` diagnostics, and
qualified non-Boolean WHERE/NOT/AND/OR operands use `42804` with native-backed
messages and ordered detail. SQL string literals and NULL remain contextual
UNKNOWN literals; an embedded string parameter is a known TEXT input.

A column summary requires an exact namespace match or a verified single-source
qualifier. Missing, nonlocal or unrepresented direct-JOIN metadata leaves that
expression with the established binder; it must not inherit another source
column's type through an unqualified fallback. Independent selected parameter
checks still apply. This finite fold does not provide a complete joined or outer
input-signature resolver.

Slots inferred through nonselected operators retain their existing consistency
and error path. Arithmetic results and CASE output metadata remain outside the
new Boolean diagnostics; heap-free common-identity handling preserves existing
nonprimitive CASE parameter OIDs. Declared primitive inputs and independent
parameter slots still receive the selected checks.

### Searched CASE Boolean diagnostics

For a searched `CASE`, validate each `WHEN` condition as a Boolean context
during Parse. A qualified non-BOOLEAN input, including a declared primitive
parameter, returns PostgreSQL 18.6 SQLSTATE `42804` with
`argument of CASE/WHEN must be type boolean, not type <name>` before statement
metadata is published. Unknown parameter OID `0` or `705` continues to infer
BOOLEAN from the condition; contextual UNKNOWN literals keep their existing
rules. This increment does not change simple `CASE` operand matching,
branch/result common-type inference, or CASE output metadata.

Checked Parse, selected typed execution, Describe and EXPLAIN must consume the
same private outcome before pagination, early EXPLAIN or warm caches. Public
Vec/Option signatures and explicitly supplied zero remain compatible, while
embedded untyped NULL has separate provenance from a wire UNKNOWN declaration.
New phase frames, per-occurrence records, summaries and query-local target
handles share the controls' memory tracker, with admission before growth and
old/new allocation overlap. Existing catalog/field discovery remains its
separate input-allocation boundary. Successful metadata handoff retains only
the OID vector's transient reservation. Rejected metadata releases its scratch;
a structured semantic error retains its admitted formatting/frame ownership
until the send completes or fails. Memory denial while constructing that
private Parse error uses `53200`. Cancellation and deadline paths must retire
these owners. Retained prepared-vector ownership remains a separate obligation.

The fresh accepted 3304 baseline retains 81 cases / 162 text/binary paths and
four named recovery workflows across original37/order32/refined12. Their
matrix and raw-journal observations are unchanged from the historical baseline.
Original Jev .85/.72 remain open until their recorded obligations are resolved.
Unused-slot `42P18`, generic UNKNOWN rejection/finalization, wider callable and
operator resolution, DML/RETURNING transformation and complete resource/restart
qualification are not accepted by this finite selection. #904/#793/#791/#848/#849
remain open; the milestone's corpus gate is unchanged.

Reuse #877 for the revised profile/scope and bootstrap infrastructure, #878 for
the refreshed inventory ledger and #885 for the expanded type decision records.
Their current contents are useful foundations but do not complete the expanded
contracts. Integrate each coherent bundle serially after full validation/review.

For each runtime increment, retain red → green → refactor should_ tests and
Arrange/Act/Assert; async tests use a current-thread Tokio runtime. Run one heavy
local lane and preserve unrelated dirty work. Complete this ordered gate:

```sh
cargo build --locked
cargo test --locked
cargo clippy --workspace --all-targets --all-features -- -D warnings -D clippy::pedantic
cargo fmt --all -- --check
cntryl-tools validate-tests -f <each touched test file>
cntryl-tools validate-docs --config .cntryl/repository.toml
cntryl-tools validate-benchmarks --config .cntryl/repository.toml
cntryl-tools check-module-sizes --config .cntryl/repository.toml
```

Then complete clean adversarial review and mandatory focused probes for Jev gap
probability ≥0.30. Recheck actual repository-required GitHub rules/checks against
the final head. Under the user-selected local-first policy, the complete ordered
local gate, clean review, mandatory probes and required GitHub rules/checks must
pass; pending nonrequired hosted checks need not delay merge. Disclose those
pending checks and never report them as passed. Guard squash merge to the reviewed
head, then read back the merged commit/tree and issue state.
The milestone closes only after every in-scope corpus cell passes, every owner
and prerequisite acceptance criterion is complete and #849 resource/restart
evidence is accepted. Failed, skipped, unavailable, pending or unsupported
in-scope cells keep it open. Update support/readiness claims from that evidence
and close #791 last.

## Consolidated local delivery batch

The user selected one larger delivery PR on 2026-10-11, with small local
commits, focused red → green tests and continuous independent review. Integrate
those commits locally and run the complete ordered gate once the batch is ready.
During iteration, run cheap formatting and touched-test policy checks before
compilation, then compile related targets together where useful. Existing full
suite coverage supplies unchanged regression evidence; additional focused
witnesses address changed behavior, interactions and mandatory adversarial gaps.
Preserve raw logs with one concise acceptance matrix. Optional hosted workflows
do not delay delivery under the recorded local-first policy.

The Parse-only resource prerequisite under
[#904](https://github.com/cntryl/cassie/issues/904) and current cache/authorization
qualification under [#766](https://github.com/cntryl/cassie/issues/766) are
accepted through [PR #933](https://github.com/cntryl/cassie/pull/933), merged
2026-10-11 as `ade53364ab2bea6d951d9ed9781b9a23ddae99f8`, tree
`3afa8cae42caa350cb85f9f003b718880d052d6a`. The completed #763, #871, #886
and #882 foundations made #766 independent of the staged Parse merge. That
accepted batch changed no public API, persistent format or permission model.

| Invariants | Accepted finite qualification and remaining boundary |
| --- | --- |
| CACHE-01/02 | Existing cross-session/context and warm-cache authority controls remain qualified. |
| CACHE-03 | Two warm readers observe committed changes after a Data-applied publication error; an actual reader also sees old rows/hit before commit, and cold restart preserves new rows/epoch without provider calls. |
| CACHE-04 | Actual warm-hit to transaction/savepoint bypass controls pass; current provider callgraph boundaries remain explicit. |
| CACHE-05 | Complete embedded persisted-plan key is validated; foreign context rejects/deletes and real app execution rebuilds authoritatively. Configured byte/entry and replacement/eviction controls pass. Shared-tracker result ownership remains unimplemented. |
| AUTH-01 | Materialized portal resume rechecks current access, with denial/cancellation cleanup; streaming and existing execute/Describe controls pass. |
| AUTH-02 | Read-only CALL causes no side effects; existing network, COPY and administrative-image boundaries remain qualified. |
| AUTH-03 | Actual known/unknown/throttled wire and REST failures and bounded complete captured logs pass plaintext/stored-hash redaction checks. |
| AUTH-04 | Revoke, rotation, NOLOGIN, deletion and expiry invalidate opaque sessions through the actual REST query endpoint; images retain pgwire COPY controls under AUTH-01/02/05. |
| AUTH-05 | Existing listener, credentials, transport and admission-permit cleanup controls remain qualified. |

The complete ordered local gate passed all 27 commands: 3,898 tests, zero
failures and nine existing ignores across 35 result groups and 34 actual Cargo
executables. Independent final review accepted the exact source. Actual mandatory
Parse ownership and portal-authorization probes, plus the two final persisted-plan
and postcommit operational probes, passed without rescoring waivers. The latter
postcommit probe retained 20 actual records and 45 checks. Final rules and the
merged tree were read back; at merge, optional hosted checks were five passed,
one neutral and eight pending. Pending checks were not counted as passed.

The earlier interim Parse gate remains cancelled; its partial receipt supplies
no full-gate acceptance. Full #904/#793/#791/#848/#849 and the original parameter
occurrence/coercion gaps remain open. #933 uses `Refs #766`: its finite repairs
and configured cache-bound tests do not prove lifetime ownership of cache copies.

### Selected CACHE-05 ownership batch

On 2026-10-11 the user approved A+O from the concrete independently reviewed
proposal: immutable shared results plus originating-query physical ownership
and separate requesting-query exposure admission. The selected
[Query Result Ownership](query-result-ownership.md) contract records exact
source-breaking QueryResult, REST and EXPLAIN APIs before implementation.
It covers owned-row transfers and encoded HTTP bytes through their final frame
owner, with reservation-only leases that retain no full controls, statement-read
snapshot, cancellation, request or session scope.

New conversion/encoding allocations charge the active admitted requesting view;
old shared physical storage retains its producer lease. Required allocation/hit
admission fails with `54000` before output; optional cache insertion may skip
with `memory_budget`. Existing serialized cache-byte limits remain distinct from
retained-capacity accounting. Complete backing-container and old/new overlap
ownership is required; public return does not release a live allocation's lease.

The implementation candidate is present for the remaining #766 current-surface
acceptance, including SQL and vector execution-result cache keys and backing
directories. The selected [ownership contract](query-result-ownership.md) defines
that coverage; [performance contracts](performance-contracts.md#query-memory-and-results)
define retained-capacity and latest-query memory observations.
Small local tests-first commits retain the implementation evidence. Actual
ownership acceptance requires the final ordered gate, independent review and
guarded squash/readback; the candidate alone closes nothing. The current
disposition and completed delivery evidence are retained in
[#766](https://github.com/cntryl/cassie/issues/766).
No SQL type, AST, persistent format, transaction-error API or permission-model
change is selected. The pending transaction-error API and external stress
collector decisions remain outside this batch. Keep #766 open until all selected
criteria, actual ownership witnesses, review, required rules and merged-tree
readback are complete; do not move its mandatory tracker obligation to #849.

## Accepted primitive receive increments

The scalar registry and Boolean prerequisite is accepted as [PR #929](https://github.com/cntryl/cassie/pull/929),
commit `3077586b4d2719b9aaa15bbb095ed353c8eb0a24`, tree
`37622657c3416310efccc9eae1f3b95ba7ad2478`: full ordered gate 3,850 Rust
passes, twenty exact witnesses, 552 selected PostgreSQL comparisons, 24 additional
mixed-parameter comparisons and all fifteen fresh hosted checks. Exact merged
tree and #928 closure are verified; full parents remain open.

The [#930 bundle](https://github.com/cntryl/cassie/issues/930) was defined before
coding and completed through [PR #931](https://github.com/cntryl/cassie/pull/931),
commit `e2cf9c6818d40b28c38af4b1918abaa4d57c5ef8`, tree
`bc549e8a6bd5735da26ab90b970925de52737481`. Its full ordered gate passed
3,854 Rust tests, with 26 separate exact witnesses and all 15 fresh hosted checks;
#930 closure and exact merged-tree readback are verified. The four new
`pgwire_scalar_receive_errors` tests qualify fixed scalar metadata, temporal
error priority, rejected mutations and nested legacy scalar propagation.
Existing scalar receive errors use the registered fixed width,
validate that prefix, then classify excess bytes as `22P03`; insufficient data
remains `08P01`. Confirmed invalid DATE/TIME/TIMESTAMP range errors use `22008`
before an excess-byte error. This is Stage 1 primitive work under #904/#780,
with explicit staged input-error ownership recorded in #814. It consumes the
completed #885 records, #926 extraction and #929 registry without consuming new
temporal types, mutable catalog roots or isolation. Both approved format
contracts and the currently admitted value set remain unchanged.

Pre-#930 read-only preparation recorded 120 passes and twenty failures in the
140-case scalar probe, and 24 passes and forty failures in the 64-case temporal
priority probe. Those baseline receipts remain historical. PR #931 passed all
102 selected receive paths and all 552 prior Boolean paths; its complete pinned
corpus record remains FAILED at 695 passes, 59 failures and 4,009 pending. This
record is not a new corpus run. PostgreSQL validates an invalid temporal prefix
before checking excess bytes. Timestamp text presentation, infinity, 24:00,
large-year support and other full temporal obligations remain under #814/#904/#848.
Existing legacy-array elements delegate to the scalar receiver; #930 qualifies
the propagated error classes with explicit legacy identities and native oracle
probes. Array identity and grammar stay unchanged. The outer-container framing
change remains with #807 and its #796/#762 prerequisites.
The independent native ARM failure remains under #927. These are pending
program obligations, never architectural exceptions or passing cells.
