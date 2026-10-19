# PostgreSQL 18 compatibility test inventory (historical snapshot)

> **Historical snapshot, not an active product or backlog specification.** The
> counts and owner states below were recorded on 2026-10-15 for the former
> full-parity program. The current product uses PostgreSQL 18.6 as a comparison
> oracle only for selected behaviors. See [Product Roadmap](product-roadmap.md)
> and the [current testing guide](testing/README.md).

Refreshed **2026-10-15**. This is the compatibility owner index. The consolidated [test inventory](testing/README.md) contains every existing test and its physical owner; the [backfill ledger](testing/planned-tests.md) maps all **138 identified remaining items** to registered assertions. There are no unwritten items in that reconciled list. An authored assertion does not establish that Cassie reaches its intended path or passes it.

The inventory snapshot contains **69 open and 20 closed milestone issues**. The initial backfill and reorganization changed tests only. The user subsequently authorized runtime fixes; that implementation is in progress on the same branch. Authored changes and focused passes do not close owners or establish a full-suite pass. The 4,009 upstream corpus entries and client/workload qualification gates are separate acceptance obligations.

## Sources of truth

| Record | Scope |
| --- | --- |
| [Test organization](testing/README.md) | Directory ownership, naming, fixture boundaries and registration rules |
| [Every existing test](testing/inventory.json) | Every declaration and registered instance; generated totals are in [the summary](testing/summary.md) |
| [Reconciled backfill](testing/planned-tests.md) | Issue, inventory name, actual assertion, authoring and execution status for all 138 items |
| [Oracle corrections](testing/oracle-corrections.md) | 28 historical corrections and 54 strengthened parse-only checks |
| [Owner assertion anchors](testing/owner-assertions.md) | Finite assertion links and prerequisite limits for all 69 open owners |
| [Validation](testing/validation.md) | Commands, observed results, identity changes and prerequisite limits |
| [Former delivery dependencies](postgresql18-delivery.md) | Historical runtime implementation and qualification order; not current sequencing |

The delivery order below is the former full-parity sequence. It is retained as
history only and does not define current work order. Use the live roadmap and
milestones.

## Delivery order (historical)

1. The former program placed #819 isolation and #833 transactional catalog foundations first.
2. It placed #796 before #800 row grammar and #762 row subqueries; #762 also depended on #761, #777 and #780.
3. It placed #905 ranges before #824 constraints, then #826 table forms and #828 named row types.
4. It required routine/catalog dependencies before #910–#916.
5. It required #847 and #848 qualification before #849, with #791 to close last.

Test authoring can proceed in parallel across exclusive files. Integration, Cargo validation and later runtime delivery stay serialized. Tests whose positive controls fail at an earlier dependency remain prerequisite failures, even when the final assertion has the right PostgreSQL expectation.

## Open milestone owners

The count below is the number of reconciled remaining-list items for an owner, not its total tests. A zero means this pass retained existing coverage or a non-SQL qualification gate; it does not claim that the owner's entire acceptance contract has passed. Full existing coverage is indexed in the owner pages under `testing/inventory/`.

| Issue | Live title | Remaining-list assertions / qualification |
| --- | --- | --- |
| [#762](https://github.com/cntryl/cassie/issues/762) | Feature request: Add bounded scalar, IN, ANY and ALL subquery expressions | 10 linked items |
| [#791](https://github.com/cntryl/cassie/issues/791) | Documentation/readiness: Track PostgreSQL 18 core SQL and full PL/pgSQL parity | Existing tests retained; qualification gate |
| [#792](https://github.com/cntryl/cassie/issues/792) | Documentation/readiness: Define PostgreSQL 18 core SQL and full PL/pgSQL contracts | Existing tests retained; qualification gate |
| [#793](https://github.com/cntryl/cassie/issues/793) | Documentation/readiness: Specify extended SQL type, wire and storage contracts | Existing tests retained; qualification gate |
| [#796](https://github.com/cntryl/cassie/issues/796) | Feature request: Complete PostgreSQL cast, type-name, literal, operator and collation syntax | 1 linked items |
| [#798](https://github.com/cntryl/cassie/issues/798) | Feature request: Implement null-safe comparison predicates | Existing tests retained; runtime acceptance open |
| [#799](https://github.com/cntryl/cassie/issues/799) | Feature request: Add ILIKE, SIMILAR TO, regular-expression and text-search predicates | 4 linked items |
| [#800](https://github.com/cntryl/cassie/issues/800) | Feature request: Complete PostgreSQL relation, row and TABLESAMPLE grammar | 5 linked items |
| [#801](https://github.com/cntryl/cassie/issues/801) | Feature request: Support CTE materialization modifiers and recursive SEARCH and CYCLE clauses | 2 linked items |
| [#803](https://github.com/cntryl/cassie/issues/803) | Feature request: Extend PostgreSQL string functions and formatting syntax | Existing tests retained; runtime acceptance open |
| [#804](https://github.com/cntryl/cassie/issues/804) | Feature request: Support aggregate DISTINCT, FILTER, ordered arguments and grouping sets | Existing tests retained; runtime acceptance open |
| [#805](https://github.com/cntryl/cassie/issues/805) | Feature request: Add BOOL_AND and BOOL_OR aggregates | Existing tests retained; runtime acceptance open |
| [#806](https://github.com/cntryl/cassie/issues/806) | Feature request: Add statistical, ordered-set and window function families | 2 linked items |
| [#807](https://github.com/cntryl/cassie/issues/807) | Feature request: Add PostgreSQL array constructors, predicates and indexing | Existing tests retained; runtime acceptance open |
| [#808](https://github.com/cntryl/cassie/issues/808) | Feature request: Execute UNNEST, generate_series and table-function ordinality | 2 linked items |
| [#809](https://github.com/cntryl/cassie/issues/809) | Feature request: Implement PostgreSQL JSON operators and path predicates | 1 linked items |
| [#810](https://github.com/cntryl/cassie/issues/810) | Feature request: Add JSON inspection, construction, conversion and SQL/JSON standard functions | 2 linked items |
| [#811](https://github.com/cntryl/cassie/issues/811) | Feature request: Add ordered string, array and JSON collection aggregates | 12 linked items |
| [#812](https://github.com/cntryl/cassie/issues/812) | Feature request: Implement exact NUMERIC and DECIMAL values | 2 linked items |
| [#813](https://github.com/cntryl/cassie/issues/813) | Feature request: Add typed rounding and numeric scalar functions | 2 linked items |
| [#814](https://github.com/cntryl/cassie/issues/814) | Feature request: Implement TIMESTAMPTZ and calendar INTERVAL semantics | 2 linked items |
| [#815](https://github.com/cntryl/cassie/issues/815) | Feature request: Implement SQL clock and calendar functions | Existing tests retained; runtime acceptance open |
| [#816](https://github.com/cntryl/cassie/issues/816) | Feature request: Add bit-string, bytea, network-address, UUID and hashing types and functions | 3 linked items |
| [#817](https://github.com/cntryl/cassie/issues/817) | Enhancement: Extend PostgreSQL catalogs, introspection relations and system information functions | 30 linked items |
| [#818](https://github.com/cntryl/cassie/issues/818) | Feature request: Resolve PostgreSQL object identifier pseudo-types | Existing tests retained; runtime acceptance open |
| [#819](https://github.com/cntryl/cassie/issues/819) | Feature request: Add the selected snapshot isolation query profile | Existing tests retained; runtime acceptance open |
| [#820](https://github.com/cntryl/cassie/issues/820) | Feature request: Support row and table locking with SKIP LOCKED claims | 2 linked items |
| [#821](https://github.com/cntryl/cassie/issues/821) | Enhancement: Extend PostgreSQL session settings, prepared statements, cursors and session reset | Existing tests retained; runtime acceptance open |
| [#822](https://github.com/cntryl/cassie/issues/822) | Feature request: Add advisory locks and LISTEN/NOTIFY | Existing tests retained; runtime acceptance open |
| [#823](https://github.com/cntryl/cassie/issues/823) | Feature request: Execute MERGE, TRUNCATE, UPDATE FROM and DELETE USING statements | 2 linked items |
| [#824](https://github.com/cntryl/cassie/issues/824) | Enhancement: Complete PostgreSQL CHECK, exclusion and temporal constraints | Existing tests retained; runtime acceptance open |
| [#825](https://github.com/cntryl/cassie/issues/825) | Enhancement: Complete PostgreSQL conflict target and conditional upsert syntax | 2 linked items |
| [#826](https://github.com/cntryl/cassie/issues/826) | Feature request: Add PostgreSQL table creation, cloning and materialized view forms | Existing tests retained; runtime acceptance open |
| [#827](https://github.com/cntryl/cassie/issues/827) | Feature request: Add named enum types and typed enum values | Existing tests retained; runtime acceptance open |
| [#828](https://github.com/cntryl/cassie/issues/828) | Feature request: Add PostgreSQL composite, record and domain semantics | Existing tests retained; runtime acceptance open |
| [#829](https://github.com/cntryl/cassie/issues/829) | Feature request: Execute JSON element and recordset table functions | Existing tests retained; runtime acceptance open |
| [#830](https://github.com/cntryl/cassie/issues/830) | Feature request: Extend SQL routines with signatures and set-returning results | Existing tests retained; runtime acceptance open |
| [#831](https://github.com/cntryl/cassie/issues/831) | Feature request: Complete object ownership, privilege and row-level security DDL | Existing tests retained; runtime acceptance open |
| [#832](https://github.com/cntryl/cassie/issues/832) | Feature request: Enforce routine security context and scoped settings | Existing tests retained; runtime acceptance open |
| [#833](https://github.com/cntryl/cassie/issues/833) | Feature request: Complete transactional DDL and catalog publication | Existing tests retained; runtime acceptance open |
| [#834](https://github.com/cntryl/cassie/issues/834) | Feature request: Execute data-modifying CTEs with RETURNING relations | 2 linked items |
| [#835](https://github.com/cntryl/cassie/issues/835) | Feature request: Implement full PostgreSQL PL/pgSQL compatibility | Existing tests retained; runtime acceptance open |
| [#836](https://github.com/cntryl/cassie/issues/836) | Feature request: Add anonymous procedural blocks and parameterized dynamic SQL | Existing tests retained; runtime acceptance open |
| [#837](https://github.com/cntryl/cassie/issues/837) | Feature request: Add PostgreSQL row, statement, constraint and event triggers | Existing tests retained; runtime acceptance open |
| [#838](https://github.com/cntryl/cassie/issues/838) | Feature request: Extend database, schema and temporary relation lifecycle | Existing tests retained; runtime acceptance open |
| [#839](https://github.com/cntryl/cassie/issues/839) | Feature request: Support atomic ALTER, rename and DROP lifecycle across relations and objects | Existing tests retained; runtime acceptance open |
| [#840](https://github.com/cntryl/cassie/issues/840) | Feature request: Add LIST, RANGE and HASH partitioned relation lifecycle and routing | Existing tests retained; runtime acceptance open |
| [#841](https://github.com/cntryl/cassie/issues/841) | Enhancement: Extend sequence options, identity and value functions | Existing tests retained; runtime acceptance open |
| [#842](https://github.com/cntryl/cassie/issues/842) | Enhancement: Implement PostgreSQL index DDL and Midge-backed SQL maintenance | Existing tests retained; runtime acceptance open |
| [#843](https://github.com/cntryl/cassie/issues/843) | Feature request: Add an explicit built-in extension compatibility catalog | Existing tests retained; runtime acceptance open |
| [#844](https://github.com/cntryl/cassie/issues/844) | Feature request: Support the selected pgvector SQL and type identity profile | 6 linked items |
| [#845](https://github.com/cntryl/cassie/issues/845) | Feature request: Persist SQL object comments and identity metadata | Existing tests retained; runtime acceptance open |
| [#846](https://github.com/cntryl/cassie/issues/846) | Enhancement: Complete PostgreSQL COPY and migration I/O forms | Existing tests retained; runtime acceptance open |
| [#847](https://github.com/cntryl/cassie/issues/847) | Enhancement: Qualify generated PostgreSQL ORM queries against the dialect profile | Existing tests retained; qualification gate |
| [#848](https://github.com/cntryl/cassie/issues/848) | Documentation/readiness: Qualify the complete generic SQL dialect corpus | Existing tests retained; qualification gate |
| [#849](https://github.com/cntryl/cassie/issues/849) | Documentation/readiness: Establish dialect workload resource and restart profiles | Existing tests retained; qualification gate |
| [#905](https://github.com/cntryl/cassie/issues/905) | Feature request: Add PostgreSQL range and multirange types | 2 linked items |
| [#906](https://github.com/cntryl/cassie/issues/906) | Feature request: Add PostgreSQL XML values and SQL/XML functions | 2 linked items |
| [#907](https://github.com/cntryl/cassie/issues/907) | Feature request: Add PostgreSQL geometric types and functions | 3 linked items |
| [#908](https://github.com/cntryl/cassie/issues/908) | Feature request: Add PostgreSQL money values and arithmetic | 1 linked items |
| [#909](https://github.com/cntryl/cassie/issues/909) | Feature request: Implement PostgreSQL table inheritance | 2 linked items |
| [#910](https://github.com/cntryl/cassie/issues/910) | Feature request: Implement SQL-defined aggregate objects | 3 linked items |
| [#911](https://github.com/cntryl/cassie/issues/911) | Feature request: Implement SQL-defined casts and conversions | 7 linked items |
| [#912](https://github.com/cntryl/cassie/issues/912) | Feature request: Implement SQL-defined operators and operator families | 4 linked items |
| [#913](https://github.com/cntryl/cassie/issues/913) | Feature request: Implement PostgreSQL SQL rewrite rules | 5 linked items |
| [#914](https://github.com/cntryl/cassie/issues/914) | Feature request: Implement PostgreSQL extended statistics | 1 linked items |
| [#915](https://github.com/cntryl/cassie/issues/915) | Feature request: Implement local two-phase transactions and recovery | Existing tests retained; runtime acceptance open |
| [#916](https://github.com/cntryl/cassie/issues/916) | Feature request: Implement Midge-backed PostgreSQL large objects | 8 linked items |
| [#927](https://github.com/cntryl/cassie/issues/927) | Bug: Native ARM benchmark artifacts lack a canonical CPU identity | Existing tests retained; qualification gate |

## Explicit compatibility profile policies

The user approved standard PostgreSQL array OIDs and rejection of Cassie legacy OIDs; `digest()` requires `CREATE EXTENSION pgcrypto`. Legacy OID rejection uses the Cassie profile's `0A000`; PostgreSQL itself reports `XX000` for an unknown catalog OID, so that rejection is not claimed as common SQLSTATE parity.

For #817 the user approved `0A000` for physical file/control and server-management helpers that cannot truthfully represent Midge storage. Commit-timestamp helpers must report PostgreSQL's default `55000` when tracking is off. Catalog rows, SQL statistics restoration and authorized backend cancellation/termination remain positive behavior requirements. Tests include typed-NULL, registration or privilege controls so unrelated failures cannot satisfy those rejection assertions.

On 2026-10-15 the user selected actual Midge storage sizes for size functions.
Tests must use authoritative Midge storage accounting rather than PostgreSQL
heap, index or TOAST page constants. PostgreSQL-only fork and TOAST operations
follow the approved `0A000` rejection policy. This decision does not establish
implemented size functions or passing storage-accounting evidence.

## Qualification still required

No pending, ignored or reference-only corpus/client/profile observation counts as a Cassie pass. #847 pinned Prisma execution, #848 owner/form/path execution cells, #849 immutable workload/resource/restart profiles and #927 real host/artifact admission remain open gates. The six pgvector tests are authored with dynamic catalog OIDs and an explicit extension dependency; the local PostgreSQL installation does not have the pinned pgvector extension, so this pass does not claim a pgvector oracle run.

### #849 final workload and resource profile matrix

These are qualification gates, not substitutes for the issue-specific SQL
regressions above. The repository performance contract already defines the
available scale axes: representative Tier 3 cases at 100k rows; Tier 4 real
client paths at 10k rows; Tier 5 applicable row counts at 10k, 100k, and 250k,
client counts at 1, 2, 4, 8, and 16, and worker counts at 1, 2, and 4; and the
two Tier 6 one-hour profiles at 100k and 10k. Apply only the axes relevant to
each workload, and retain the exact fixture, configuration, toolchain, client,
and immutable Cassie revision in the artifact.

| Workload category | Proposed test or profile name | Required observation | Existing evidence anchor and remaining gap |
| --- | --- | --- | --- |
| Relational and reporting | `perf.postgres18.relational_reporting.{10k,100k,250k}`; **should_qualify_postgres18_relational_reporting_at_declared_scales** | Representative filtered, joined, grouped, ordered, and paginated results; values, NULLs, descriptors, selected paths, latency, capacity, memory, and cancellation at each applicable scale. | Existing anchors include `perf.read_path.mixed_order.100k`, `perf.read_path.vectorized_join.100k`, and Tier 5 relational/join curves. They are not a completed PostgreSQL 18 reporting profile. |
| JSON expansion | `perf.postgres18.json_expansion.{10k,100k,250k}`; **should_qualify_postgres18_json_expansion_resource_and_cancel_bounds** | Stable row/value/order results and PostgreSQL errors for representative JSON expansion; peak accounted memory, bounded expansion, cancellation latency, and no partial result publication. | #809 owns SQL/JSON semantics and cancellation cases. No dialect-specific scale profile is recorded. |
| Mutation and publication | `perf.postgres18.mutation_publication.{10k,100k,250k}`; **should_preserve_acknowledged_mutations_and_publication_after_restart** | Insert/update/delete and conflict workload correctness; acknowledged versus unacknowledged outcomes; write amplification, contention, cancellation/rollback, index or derived-state publication, and cold-restart readback. | Existing column DML, replay/rebuild, and lifecycle profiles cover narrower paths. They do not form the requested cross-dialect mutation profile. |
| Coordination and contention | `perf.postgres18.coordination.clients.{1,2,4,8,16}`; **should_qualify_postgres18_coordination_under_contention_and_restart** | Two-session transaction/lock/notification outcomes, progress and contention behavior, capacity at applicable client/worker counts, cancellation cleanup, and acknowledged-state recovery. | #819/#820/#822 own semantic isolation, prepared transaction, advisory lock, and notification tests. Existing client saturation does not exercise this full coordination workload. |
| Client workloads | `perf.postgres18.client.pgwire.10k`; **should_qualify_pinned_postgres_clients_against_postgres18_profiles** | Execute representative prepared and parameterized SQL through actual pinned clients; retain result/descriptor/error evidence and client versions. Include Prisma 7 (#847) and pg_dump/restore (#846) only when their pinned prerequisites are ready. | Existing Tier 4 pgwire/HTTP and Tier 5 transport curves are anchors. The ignored/pending Prisma test and unqualified pg_dump/restore are not passes. |
| Cross-profile resource and cancellation | **should_bound_memory_and_cancel_without_partial_output_or_mutation_for_each_profile** | Every applicable workload records peak accounted query memory, workers and leaked workers separately, reservations after completion, cancellation latency/SQLSTATE, and absence of partial rows or mutation. | Tier 3/4/6 contracts already require some of these observations for selected owners; the complete dialect workload set has no consolidated readback. |
| Cross-profile restart and immutable evidence | **should_recover_acknowledged_dialect_state_after_cold_restart** | On the same immutable revision, verify acknowledged state and publication after cold restart; preserve raw artifacts and separate semantic completion from production promotion. | Existing per-owner restart tests remain finite evidence. #849 still requires one linked final profile record; #781 owns the final exact revision/image/profile/client/known-limit ledger. |

The current repository benchmark anchors are `perf.scale.query.relational.*`,
`perf.scale.query.join.*`, `perf.scale.query.column.*`,
`perf.scale.transport.clients.*`, `perf.soak.mixed.100k`, and
`perf.soak.transport_lifecycle.10k`. Their presence establishes available
harnesses and finite scenarios only; it does not establish that the workload
matrix above has run or passed for PostgreSQL 18.6.


## Closed milestone owners: retain finite regression evidence

These owners are not counted as open work. Keep their exact regression evidence linked when finalizing the inventory; a closed issue proves only its finite acceptance slice. In particular, #904 retains type-resolution, array OID, aggregate result identity, catalog publication, and restart carryovers.

| Closed issues | Finite evidence to retain |
| --- | --- |
| [#794](https://github.com/cntryl/cassie/issues/794), [#795](https://github.com/cntryl/cassie/issues/795), [#797](https://github.com/cntryl/cassie/issues/797), [#802](https://github.com/cntryl/cassie/issues/802) | Compact operator tokenization; relation alias/qualified references; LIMIT/OFFSET parameters; NULLIF/GREATEST/LEAST. Relevant parser, relational, parameter, and conditional-expression tests remain regression anchors. |
| [#850](https://github.com/cntryl/cassie/issues/850), [#857](https://github.com/cntryl/cassie/issues/857) | Mixed-numeric COALESCE normalization; retained memory for existing plain CTE materialization. Preserve exact input cases and memory-release assertions. |
| [#864](https://github.com/cntryl/cassie/issues/864), [#866](https://github.com/cntryl/cassie/issues/866), [#868](https://github.com/cntryl/cassie/issues/868), [#869](https://github.com/cntryl/cassie/issues/869), [#870](https://github.com/cntryl/cassie/issues/870) | Correlated EXISTS in SELECT, delimited dotted fields, HAVING/order/join/window, DML RETURNING, and nested ancestor scope. Retain finite context-specific query/mutation tests; #922/#923 remain separate planning behavior. |
| [#875](https://github.com/cntryl/cassie/issues/875), [#881](https://github.com/cntryl/cassie/issues/881) | Reject trailing ignored INSERT text; qualify skipped-conflict INSERT RETURNING on selected ingress paths only. Do not generalize #881 to all mutation/RETURNING cases. |
| [#904](https://github.com/cntryl/cassie/issues/904) | Bounded shared type/catalog runtime slices. Carryover remains for #807 array OIDs, #804/#812 SUM/AVG(BIGINT) identity, named identities, publication/rollback/restart, and cache invalidation. |
| [#922](https://github.com/cntryl/cassie/issues/922), [#923](https://github.com/cntryl/cassie/issues/923), [#924](https://github.com/cntryl/cassie/issues/924), [#925](https://github.com/cntryl/cassie/issues/925) | Planning-time constant errors; irrelevant EXISTS target elision; qualified SELECT labels; private projection catalog extraction. Preserve behavior tests separately from structural extraction. |
| [#928](https://github.com/cntryl/cassie/issues/928), [#930](https://github.com/cntryl/cassie/issues/930) | Selected Boolean binary reception and fixed scalar binary receive errors. These finite receive cells do not prove named array identity or all type codecs. |

## External gates and dependencies

State was refreshed with `gh` on 2026-10-15. Open means the item remains a dependency or qualification obligation; it does not mean every row requires a new Rust test.

| Issue | State | Inventory relationship |
| --- | --- | --- |
| [#747](https://github.com/cntryl/cassie/issues/747) | Open | Query-engine invariant/capability gaps inform selected execution cases. |
| [#761](https://github.com/cntryl/cassie/issues/761) | Open | Relational semantics gate; blocks complete #762 subquery acceptance. |
| [#768](https://github.com/cntryl/cassie/issues/768) | Open | Column layout/codec/publication and native benchmark evidence; related to #842 and #927. |
| [#777](https://github.com/cntryl/cassie/issues/777) | Open | Session catalog and named-client qualification; prerequisite for client/catalog rows. |
| [#779](https://github.com/cntryl/cassie/issues/779) | Open | Cross-path query invariants; required final corpus dimension. |
| [#780](https://github.com/cntryl/cassie/issues/780) | Open | PostgreSQL wire state-machine qualification; required for SQLSTATE, portals, parameters, and error recovery. |
| [#781](https://github.com/cntryl/cassie/issues/781) | Open | Query-engine release/profile acceptance; feeds #849. |
| [#8](https://github.com/cntryl/cassie/issues/8), [#29](https://github.com/cntryl/cassie/issues/29) | Open | Operational thresholds and diagnostics promotion; required by #849 and #927 evidence. |
| [#771](https://github.com/cntryl/cassie/issues/771), [#776](https://github.com/cntryl/cassie/issues/776) | Closed | Storage recovery and vector identity foundations are finite prerequisites; closure does not expand their accepted scope. |

## Corpus and cross-cutting acceptance

The source-derived inventory at tests/corpus/postgresql18/inventory.json contains **4,009 entries** and is marked incomplete. The recorded upstream inventory has every entry marked outcome pending: 3,805 need owner review, 167 need form expansion, and 37 need cross-product classification. This corpus is the #848 case-level source; this issue ledger does not duplicate generated forms or turn reference observations into Cassie passes. See [the corpus contract](postgresql18-corpus.md).

For each applicable owner, inventory the needed axes explicitly:

- Result values, NULL distinctions, row bags versus declared order, and duplicate multiplicity.
- OIDs, typmods, names, parameter inference, OID 0, text/binary Bind, and result formats over pgwire.
- Intended SQLSTATE and command tag for failures, including recovery after Parse/Bind/Execute errors.
- Real table-backed predicates, joins, grouping, ordering, DML, indexes, and prepared statements; constant-only SELECTs do not establish those paths.
- Transaction visibility, concurrency, rollback/savepoints, cancellation, resource bounds, persistence, and restart for owners whose acceptance requires them.
- Named-client and pg_dump/restore evidence under #847/#848. Workload, cancellation-latency, capacity, and restart profiles under #849.

## Closed historical PostgreSQL-facing issues

Retain and map existing regression evidence for the screened history below. Do not reopen a historical owner without new PostgreSQL 18.6 failure evidence:

#3, #18, #110, #123, #251–#252, #258–#261, #276, #278, #302, #306, #309, #347–#349, #353–#356, #361–#367, #369, #371, #374, #378, #380, #382, #389, #392–#396, #398–#399, #403, #405–#431, #438, #447–#449, #451–#475, #477–#503, #505–#511, #513–#521, #522–#546, #549–#565, #570, #636–#637, #646, #655, #751–#752, #755, #760, and #763.

These cover prior SQL, types, result behavior, DDL, transactions, catalogs, pgwire, and named-client regressions. Their tests are finite historical evidence, not a substitute for a still-open owner or #848 cell.
