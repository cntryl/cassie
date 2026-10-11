# PostgreSQL 18 parity backlog status

Live GitHub snapshot: 2026-10-10T20:27:48.005802+00:00. This ledger belongs to [#791](https://github.com/cntryl/cassie/issues/791) and records ownership/disposition, not implementation or compatibility proof. [Delivery](postgresql18-delivery.md) and the [dialect profile](sql-dialect-profile.md) define the expanded target and dependency policy.

The [Postgres SQL feature parity milestone](https://github.com/cntryl/cassie/milestone/6) has **70 open and 12 closed issues** in this snapshot. The original user-plan inventory was 57 open/12 closed; thirteen new owners #904–#916 explain the expansion. Earlier decimal/array/temporal/procedural slices remain intermediate. Only the recorded architectural exceptions may be excluded.

## Milestone inventory

| Issue | Title | State |
| --- | --- | --- |
| [#762](https://github.com/cntryl/cassie/issues/762) | Feature request: Add bounded scalar, IN, ANY and ALL subquery expressions | Open |
| [#791](https://github.com/cntryl/cassie/issues/791) | Documentation/readiness: Track PostgreSQL 18 core SQL and full PL/pgSQL parity | Open |
| [#792](https://github.com/cntryl/cassie/issues/792) | Documentation/readiness: Define PostgreSQL 18 core SQL and full PL/pgSQL contracts | Open |
| [#793](https://github.com/cntryl/cassie/issues/793) | Documentation/readiness: Specify extended SQL type, wire and storage contracts | Open |
| [#794](https://github.com/cntryl/cassie/issues/794) | Feature request: Tokenize PostgreSQL operators independently of whitespace | Closed |
| [#795](https://github.com/cntryl/cassie/issues/795) | Feature request: Bind ordinary relation aliases and qualified references | Closed |
| [#796](https://github.com/cntryl/cassie/issues/796) | Feature request: Complete PostgreSQL cast, type-name, literal, operator and collation syntax | Open |
| [#797](https://github.com/cntryl/cassie/issues/797) | Feature request: Bind parameters and expressions in LIMIT and OFFSET | Closed |
| [#798](https://github.com/cntryl/cassie/issues/798) | Feature request: Implement null-safe comparison predicates | Open |
| [#799](https://github.com/cntryl/cassie/issues/799) | Feature request: Add ILIKE, SIMILAR TO, regular-expression and text-search predicates | Open |
| [#800](https://github.com/cntryl/cassie/issues/800) | Feature request: Complete PostgreSQL relation, row and TABLESAMPLE grammar | Open |
| [#801](https://github.com/cntryl/cassie/issues/801) | Feature request: Support CTE materialization modifiers and recursive SEARCH and CYCLE clauses | Open |
| [#802](https://github.com/cntryl/cassie/issues/802) | Feature request: Add NULLIF, GREATEST and LEAST expressions | Closed |
| [#803](https://github.com/cntryl/cassie/issues/803) | Feature request: Extend PostgreSQL string functions and formatting syntax | Open |
| [#804](https://github.com/cntryl/cassie/issues/804) | Feature request: Support aggregate DISTINCT, FILTER, ordered arguments and grouping sets | Open |
| [#805](https://github.com/cntryl/cassie/issues/805) | Feature request: Add BOOL_AND and BOOL_OR aggregates | Open |
| [#806](https://github.com/cntryl/cassie/issues/806) | Feature request: Add statistical, ordered-set and window function families | Open |
| [#807](https://github.com/cntryl/cassie/issues/807) | Feature request: Add PostgreSQL array constructors, predicates and indexing | Open |
| [#808](https://github.com/cntryl/cassie/issues/808) | Feature request: Execute UNNEST, generate_series and table-function ordinality | Open |
| [#809](https://github.com/cntryl/cassie/issues/809) | Feature request: Implement PostgreSQL JSON operators and path predicates | Open |
| [#810](https://github.com/cntryl/cassie/issues/810) | Feature request: Add JSON inspection, construction, conversion and SQL/JSON standard functions | Open |
| [#811](https://github.com/cntryl/cassie/issues/811) | Feature request: Add ordered string, array and JSON collection aggregates | Open |
| [#812](https://github.com/cntryl/cassie/issues/812) | Feature request: Implement exact NUMERIC and DECIMAL values | Open |
| [#813](https://github.com/cntryl/cassie/issues/813) | Feature request: Add typed rounding and numeric scalar functions | Open |
| [#814](https://github.com/cntryl/cassie/issues/814) | Feature request: Implement TIMESTAMPTZ and calendar INTERVAL semantics | Open |
| [#815](https://github.com/cntryl/cassie/issues/815) | Feature request: Implement SQL clock and calendar functions | Open |
| [#816](https://github.com/cntryl/cassie/issues/816) | Feature request: Add bit-string, bytea, network-address, UUID and hashing types and functions | Open |
| [#817](https://github.com/cntryl/cassie/issues/817) | Enhancement: Extend PostgreSQL catalogs, introspection relations and system information functions | Open |
| [#818](https://github.com/cntryl/cassie/issues/818) | Feature request: Resolve PostgreSQL object identifier pseudo-types | Open |
| [#819](https://github.com/cntryl/cassie/issues/819) | Feature request: Add the selected snapshot isolation query profile | Open |
| [#820](https://github.com/cntryl/cassie/issues/820) | Feature request: Support row and table locking with SKIP LOCKED claims | Open |
| [#821](https://github.com/cntryl/cassie/issues/821) | Enhancement: Extend PostgreSQL session settings, prepared statements, cursors and session reset | Open |
| [#822](https://github.com/cntryl/cassie/issues/822) | Feature request: Add advisory locks and LISTEN/NOTIFY | Open |
| [#823](https://github.com/cntryl/cassie/issues/823) | Feature request: Execute MERGE, TRUNCATE, UPDATE FROM and DELETE USING statements | Open |
| [#824](https://github.com/cntryl/cassie/issues/824) | Enhancement: Complete PostgreSQL CHECK, exclusion and temporal constraints | Open |
| [#825](https://github.com/cntryl/cassie/issues/825) | Enhancement: Complete PostgreSQL conflict target and conditional upsert syntax | Open |
| [#826](https://github.com/cntryl/cassie/issues/826) | Feature request: Add PostgreSQL table creation, cloning and materialized view forms | Open |
| [#827](https://github.com/cntryl/cassie/issues/827) | Feature request: Add named enum types and typed enum values | Open |
| [#828](https://github.com/cntryl/cassie/issues/828) | Feature request: Add PostgreSQL composite, record and domain semantics | Open |
| [#829](https://github.com/cntryl/cassie/issues/829) | Feature request: Execute JSON element and recordset table functions | Open |
| [#830](https://github.com/cntryl/cassie/issues/830) | Feature request: Extend SQL routines with signatures and set-returning results | Open |
| [#831](https://github.com/cntryl/cassie/issues/831) | Feature request: Complete object ownership, privilege and row-level security DDL | Open |
| [#832](https://github.com/cntryl/cassie/issues/832) | Feature request: Enforce routine security context and scoped settings | Open |
| [#833](https://github.com/cntryl/cassie/issues/833) | Feature request: Complete transactional DDL and catalog publication | Open |
| [#834](https://github.com/cntryl/cassie/issues/834) | Feature request: Execute data-modifying CTEs with RETURNING relations | Open |
| [#835](https://github.com/cntryl/cassie/issues/835) | Feature request: Implement full PostgreSQL PL/pgSQL compatibility | Open |
| [#836](https://github.com/cntryl/cassie/issues/836) | Feature request: Add anonymous procedural blocks and parameterized dynamic SQL | Open |
| [#837](https://github.com/cntryl/cassie/issues/837) | Feature request: Add PostgreSQL row, statement, constraint and event triggers | Open |
| [#838](https://github.com/cntryl/cassie/issues/838) | Feature request: Add session-local temporary tables and namespaces | Open |
| [#839](https://github.com/cntryl/cassie/issues/839) | Feature request: Support atomic ALTER, rename and DROP lifecycle across relations and objects | Open |
| [#840](https://github.com/cntryl/cassie/issues/840) | Feature request: Add LIST, RANGE and HASH partitioned relation lifecycle and routing | Open |
| [#841](https://github.com/cntryl/cassie/issues/841) | Enhancement: Extend sequence options, identity and value functions | Open |
| [#842](https://github.com/cntryl/cassie/issues/842) | Enhancement: Implement PostgreSQL index DDL and Midge-backed SQL maintenance | Open |
| [#843](https://github.com/cntryl/cassie/issues/843) | Feature request: Add an explicit built-in extension compatibility catalog | Open |
| [#844](https://github.com/cntryl/cassie/issues/844) | Feature request: Support the selected pgvector SQL and type identity profile | Open |
| [#845](https://github.com/cntryl/cassie/issues/845) | Feature request: Persist SQL object comments and identity metadata | Open |
| [#846](https://github.com/cntryl/cassie/issues/846) | Enhancement: Complete PostgreSQL COPY and migration I/O forms | Open |
| [#847](https://github.com/cntryl/cassie/issues/847) | Enhancement: Qualify generated PostgreSQL ORM queries against the dialect profile | Open |
| [#848](https://github.com/cntryl/cassie/issues/848) | Documentation/readiness: Qualify the complete generic SQL dialect corpus | Open |
| [#849](https://github.com/cntryl/cassie/issues/849) | Documentation/readiness: Establish dialect workload resource and restart profiles | Open |
| [#850](https://github.com/cntryl/cassie/issues/850) | Bug: Normalize mixed numeric COALESCE values before downstream arithmetic | Closed |
| [#857](https://github.com/cntryl/cassie/issues/857) | Bug: Retain query memory for existing plain CTE materialization | Closed |
| [#864](https://github.com/cntryl/cassie/issues/864) | Bug: Evaluate correlated EXISTS in SELECT for each outer row | Closed |
| [#866](https://github.com/cntryl/cassie/issues/866) | Bug: Preserve delimited dotted fields in correlated Boolean scope | Closed |
| [#868](https://github.com/cntryl/cassie/issues/868) | Bug: Evaluate correlated EXISTS in HAVING, ordering, joins and windows | Closed |
| [#869](https://github.com/cntryl/cassie/issues/869) | Bug: Resolve EXISTS in DML RETURNING expressions | Open |
| [#870](https://github.com/cntryl/cassie/issues/870) | Bug: Preserve ancestor row scope through nested EXISTS | Closed |
| [#875](https://github.com/cntryl/cassie/issues/875) | Bug: Reject ignored text after INSERT destination columns | Closed |
| [#881](https://github.com/cntryl/cassie/issues/881) | Enhancement: Qualify skipped-conflict INSERT RETURNING across selected ingress paths | Closed |
| [#904](https://github.com/cntryl/cassie/issues/904) | Feature request: Implement the shared PostgreSQL type and catalog runtime | Open |
| [#905](https://github.com/cntryl/cassie/issues/905) | Feature request: Add PostgreSQL range and multirange types | Open |
| [#906](https://github.com/cntryl/cassie/issues/906) | Feature request: Add PostgreSQL XML values and SQL/XML functions | Open |
| [#907](https://github.com/cntryl/cassie/issues/907) | Feature request: Add PostgreSQL geometric types and functions | Open |
| [#908](https://github.com/cntryl/cassie/issues/908) | Feature request: Add PostgreSQL money values and arithmetic | Open |
| [#909](https://github.com/cntryl/cassie/issues/909) | Feature request: Implement PostgreSQL table inheritance | Open |
| [#910](https://github.com/cntryl/cassie/issues/910) | Feature request: Implement SQL-defined aggregate objects | Open |
| [#911](https://github.com/cntryl/cassie/issues/911) | Feature request: Implement SQL-defined casts and conversions | Open |
| [#912](https://github.com/cntryl/cassie/issues/912) | Feature request: Implement SQL-defined operators and operator families | Open |
| [#913](https://github.com/cntryl/cassie/issues/913) | Feature request: Implement PostgreSQL SQL rewrite rules | Open |
| [#914](https://github.com/cntryl/cassie/issues/914) | Feature request: Implement PostgreSQL extended statistics | Open |
| [#915](https://github.com/cntryl/cassie/issues/915) | Feature request: Implement local two-phase transactions and recovery | Open |
| [#916](https://github.com/cntryl/cassie/issues/916) | Feature request: Implement Midge-backed PostgreSQL large objects | Open |

## External prerequisite inventory

These owners share this program's top priority and retain their original milestones. Their source/test acceptance remains separate from final #848/#849 integration.

| Issue | Milestone | State |
| --- | --- | --- |
| [#8](https://github.com/cntryl/cassie/issues/8) — Documentation/readiness: Establish operational thresholds for production profiles | Query engine 5: Compatibility and release evidence | Open |
| [#29](https://github.com/cntryl/cassie/issues/29) — Enhancement: Promote operational metadata and local capacity diagnostics | Query engine 5: Compatibility and release evidence | Open |
| [#761](https://github.com/cntryl/cassie/issues/761) — Documentation/readiness: Qualify relational semantics across row and vector kernels | Query engine 3: Relational vector kernels | Open |
| [#764](https://github.com/cntryl/cassie/issues/764) — Enhancement: Qualify ANN selection and vector index publication recovery | Query engine 4: Storage and retrieval qualification | Open |
| [#765](https://github.com/cntryl/cassie/issues/765) — Enhancement: Qualify full-text and hybrid retrieval visibility and scoring | Query engine 4: Storage and retrieval qualification | Open |
| [#766](https://github.com/cntryl/cassie/issues/766) — Enhancement: Qualify cache context and authorization ownership | Query engine 4: Storage and retrieval qualification | Open |
| [#767](https://github.com/cntryl/cassie/issues/767) — Enhancement: Probe REST vector cancellation, generations and cache invalidation | Query engine 4: Storage and retrieval qualification | Open |
| [#768](https://github.com/cntryl/cassie/issues/768) — Enhancement: Qualify column layouts, codecs and generation publication | Query engine 4: Storage and retrieval qualification | Open |
| [#769](https://github.com/cntryl/cassie/issues/769) — Enhancement: Qualify constraints and mutation identity across ingress paths | Query engine 4: Storage and retrieval qualification | Open |
| [#773](https://github.com/cntryl/cassie/issues/773) — Enhancement: Qualify projection and rollup substitution across typed execution | Query engine 4: Storage and retrieval qualification | Open |
| [#774](https://github.com/cntryl/cassie/issues/774) — Enhancement: Qualify portable relational kernels and optional SIMD dispatch | Query engine 4: Storage and retrieval qualification | Open |
| [#775](https://github.com/cntryl/cassie/issues/775) — Enhancement: Qualify typed time-series pruning and retention | Query engine 4: Storage and retrieval qualification | Open |
| [#777](https://github.com/cntryl/cassie/issues/777) — Documentation/readiness: Qualify session catalogs and named client workflows | Query engine 5: Compatibility and release evidence | Open |
| [#778](https://github.com/cntryl/cassie/issues/778) — Enhancement: Qualify typed worker, cache, feedback and diagnostics boundaries | Query engine 5: Compatibility and release evidence | Open |
| [#779](https://github.com/cntryl/cassie/issues/779) — Enhancement: Complete cross-path query invariant acceptance | Query engine 5: Compatibility and release evidence | Open |
| [#780](https://github.com/cntryl/cassie/issues/780) — Documentation/readiness: Qualify the bounded PostgreSQL wire state machine | Query engine 5: Compatibility and release evidence | Open |
| [#781](https://github.com/cntryl/cassie/issues/781) — Documentation/readiness: Consolidate query-engine release and profile acceptance | Query engine 5: Compatibility and release evidence | Open |

## Dependency and integration boundary

Follow stages 0–6 and the owner dependencies in [Delivery](postgresql18-delivery.md). #869 precedes #761; row grammar #800 precedes row-subquery acceptance #762; #762 → #777 → #780 qualifies existing catalog/client/wire foundations. #904 consumes complete #793 records, with oversized catalog extraction before extension. Built-in ranges #905 precede exclusion/temporal constraints #824, then table foundation #826 and composite/domain #828. Custom range routine/index integration remains a later obligation. A closed issue alone is insufficient: record the completed commit, exact tests, result and remaining integration obligations before consuming a staged prerequisite.

Final #848 qualification includes every old owner and #904–#916. Every failed, skipped, unavailable, pending or unsupported in-scope cell remains open. #847 clients, #779 cross-path acceptance, #8 → #29 → #781 operational/release acceptance and #849 resource/restart evidence remain final obligations. Preserve external milestones and completed foundations. #791 closes last.

PostgreSQL reference probes during #869 qualification added stage-2 owners
[#922](https://github.com/cntryl/cassie/issues/922) for planning-time constant
expression errors and [#923](https://github.com/cntryl/cassie/issues/923) for
irrelevant EXISTS target expressions. They remain final #761/#848 obligations;
the earlier inventory is a timestamped snapshot, not a claim that no new gaps
can be found.

## Completed staged foundations

These merged commits were read back through GitHub and verified as ancestors of baseline `769d4db8fb7e60f0a29fade73137433a20e5a817`. Test results below are the PRs' historical immutable qualification records, not reruns on this ledger head. Consume only their demonstrated slice; complete broader PostgreSQL semantics under the remaining owners.

| Owner | Completed commit / PR | Recorded tests and result |
| --- | --- | --- |
| #794 | [`3b03dd4db34ac88f533501d0484ecdc3cf223a2e`](https://github.com/cntryl/cassie/pull/852) | `cargo test --locked --test parser_types`; full gate at that PR |
| #795/#797 | [`7977c848775ff4b776dbf40bda2bbb9893aac25d`](https://github.com/cntryl/cassie/pull/858) | Full `cargo test --locked`: 3,421 passed, 0 failed, 9 ignored at tested publication source; alias/pagination refinements and exact merge-tree equivalence recorded |
| #802/#850 | [`a07ff562dea8e5608aee129f5902a2dee29cac1a`](https://github.com/cntryl/cassie/pull/853) | `cargo test --locked --test sql_queries coalesce -- --nocapture`; `cargo test --locked --test pgwire_extended coalesce_promotion:: -- --nocapture`; full gate |
| #758 | [`06195f0fc9c1e63a5236903976c07f480176dd8a`](https://github.com/cntryl/cassie/pull/855) | Typed aggregate/typed_batch/source_join units and `cargo test --locked --test sql_queries typed_join:: -- --nocapture`; full gate |
| #763/#864/#866 | [`ce72a23f2d0a3cdb9a811107aec7629151fcd319`](https://github.com/cntryl/cassie/pull/871) | `cargo test --locked --no-fail-fast`: 3,756 passed, 0 failed, 9 ignored; shared statement-visibility and correlated-output witnesses |
| #771 | [`f510d76910a0c1bb192e4e6ab8d267c821f0215b`](https://github.com/cntryl/cassie/pull/854) | `cargo test --locked --test storage_indexes database_image_generation:: -- --nocapture`; layout-marker admission and full gate |
| #776 | [`4545f9a08b24d2e947d6d99a4d407a9ea8ac85f5`](https://github.com/cntryl/cassie/pull/856) | Full `cargo test --locked`: 3,378 passed, 0 failed, 9 ignored; vector metric/cross-path witnesses |
| #857 | [`e3bc2974da6f8f5c60cc73afa9f3c94553dfeee6`](https://github.com/cntryl/cassie/pull/862) | `cargo test --locked --no-fail-fast`: 3,574 passed, 0 failed, 9 ignored at final frozen source; historical failed/cancelled attempts remain distinct |
| #870 | [`700cbad7bc87801b51eceb0b532451d0dc021d1b`](https://github.com/cntryl/cassie/pull/872) | Full `cargo test --locked --no-fail-fast`: 3,765 passed, 0 failed, 9 ignored; nested EXISTS witnesses |
| #868 | [`3ccb302c4d6b9f97dc033e90a53c2930e663086c`](https://github.com/cntryl/cassie/pull/873) | `cargo test --locked --test relational_qualification -- --nocapture`: 57 passed, 0 failed; relational-phase EXISTS and full-gate record |
| #875 | [`cd6e3783a0b270a10b8c1379084bd27ad0826447`](https://github.com/cntryl/cassie/pull/891) | Four INSERT destination-tail controls passed; exact-head `cargo test --locked`: 3,797 passed, 0 failed, 9 ignored |
| #881 | [`059bc453c4dd8650f21b57b482b2f52433e8ecdc`](https://github.com/cntryl/cassie/pull/888) | Three exact-head skipped-conflict RETURNING wire witnesses passed; full `cargo test --locked`: 3,797 passed, 0 failed, 9 ignored |

Remaining integration obligations include #761/#762/#768/#769/#777/#779/#780/#781 and every expanded dialect/type/catalog/client/operational cell; completed foundation rows do not close those owners.

## Existing work to reuse

| PR | Selected revision | Disposition |
| --- | --- | --- |
| [#877](https://github.com/cntryl/cassie/pull/877) | `0e8bcce46c2be9452d7bc707850032c4c663a818` | Merged by checked squash at `2345e9025d937b6479ee5bdd6e13091537292e5f`; merged tree equals the validated head. Final full local gate and fourteen hosted checks passed; no parent owner closed. |
| [#878](https://github.com/cntryl/cassie/pull/878) | `5bf186c64740b8d3f8fc56ad8a7a1accc2537585` | Reused historical ledger foundation for this expanded snapshot; the revised head requires its own complete local/hosted validation and checked integration. |
| [#885](https://github.com/cntryl/cassie/pull/885) | `699341fba52c3898582e2dcd1c46dd158fb25f54` | Earlier recommendations are staged directions. The user approved the common CRB3/identity/codec/lossless-export v3 contract on 2026-10-10 (proposal SHA256 `26c43b1718f94160be6031d9bfc32ae301575df63ab71367498394e77f04ecc6`, recorded on #793). Complete per-family records and verification still gate runtime; #793 stays open. |

These are immutable source witnesses, not current or inherited check results. Read live heads/reviews/checks before publication, merge or closure. Integrate one coherent bundle at a time.

## Bootstrap evidence and support

The stage-0 corpus inventories 4,009 pending upstream entries and hashes 370 regression/isolation inputs from immutable PostgreSQL 18.6 source. Its six bootstrap SQL cases produce sixteen simple/extended text/binary cells. An owned Cassie build whose runtime source matches `769d4db8fb7e60f0a29fade73137433a20e5a817` has thirteen passing and three failing cells: missing-column SQLSTATE 42703 on PostgreSQL versus 42601 on Cassie across all three applicable protocol modes. These failures are recorded on #848; all broad inventory/path classification remains pending. The bootstrap cannot emit complete qualification or infer instance provenance from a caller-supplied revision.

The confirmed missing-libpq preflight path currently exits without a structured receipt; #848 owns completion of unavailable/error accounting. Missing receipts remain unavailable, never passing cells.

The RETURNING increment adds `tests/corpus/postgresql18/returning.json`: twelve
cases and thirty-six simple/extended text/binary cells, including nine selected
#869 cases and three planning-gap cases owned by #922/#923. Each
mode prepares its own connections, requires successful setup and then compares
the selected result. Fixtures create tables before BEGIN; disconnect rolls back
their writes, and the next mode recreates the tables. Setup failures cannot
qualify a cell. These inputs extend the six original bootstrap cases to eighteen
cases and fifty-two cells. Execution and completion remain separate from input
inventory; failed or unavailable cells keep qualification incomplete.

No support or Production-ready promotion follows from this ledger. [Feature Support](feature-support.md), [Production Readiness](production-readiness.md), exact immutable execution receipts and current GitHub state remain authority.

## Snapshot provenance

Read through `gh api` milestone 6 issues, each focused owner, retained external issues, reused PRs and merged foundation PRs. Source baseline for this ledger is merged #877 commit `2345e9025d937b6479ee5bdd6e13091537292e5f`. Snapshot JSON and exact readbacks are retained under ignored `target/parity-evidence/`; refresh before later prioritization, closure or readiness claims. This replaces the #878 historical 2026-10-10 12:11 UTC finite-profile ledger; its original source remains available at commit `5bf186c64740b8d3f8fc56ad8a7a1accc2537585`.
