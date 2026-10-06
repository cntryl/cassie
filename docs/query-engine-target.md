# Query Engine Target 1

This is a finite delivery target for the single-node Cassie query engine. It does
not promote a feature, adopt a public batch API or select a persistent encoding.
[Feature Support](feature-support.md) remains the sole current behavior/status
owner; [Production Readiness](production-readiness.md) remains the readiness owner.
Target 1 is tracked by [#747](https://github.com/cntryl/cassie/issues/747).

The audit baseline is 5f9bfd807d4e647401fbb8a587429c48425af61e. Its inventory has
275 invariants in 24 domains: 209 mapped source/test rows, 26 architecture
obligations, 18 probe-needed rows, 15 explicit boundaries and seven confirmed-gap
rows. Those are evidence classifications, not defect counts. Four reproduced
runtime correctness/resource findings are prioritized P0; some confirmed rows
describe the same Boolean finding. A mapped test is not a fresh execution or proof
of every physical path or interleaving.

The retrieval/resource bundle merged through [#783](https://github.com/cntryl/cassie/pull/783)
at commit `f9aef65edf96278855c2d8436a82009e3cb5f9f5`;
[#748](https://github.com/cntryl/cassie/issues/748),
[#749](https://github.com/cntryl/cassie/issues/749) and
[#750](https://github.com/cntryl/cassie/issues/750) are closed. This preserves their
finite candidate-sufficiency, controlled-source/operator retention and REST vector
resource contracts without promoting broad ColumnStore or native operational readiness.

The finite contextual-Boolean repair merged through [#784](https://github.com/cntryl/cassie/pull/784)
at commit `8d94dc4a9d7f3b95ad396645deeaf9f297779bf3`, tree `4d1b16e6b383fb0689ad8cecf68ab9d7b185835b`;
[#432](https://github.com/cntryl/cassie/issues/432) is closed by that merged PR. Its documented Boolean contract is preserved.
This repair does not promote broader type, grammar, storage or native readiness surfaces.
The [ownership ledger](query-engine-invariant-ownership.md) maps all baseline
invariants to a primary child owner, dependency, independent oracle and promotion gate.

Vectorized relational execution means operators carrying batches of typed column
values, with validity, selection, row identity, ownership and bounded state.
Embedding-vector similarity retrieval is a separate capability. CPU SIMD and
specialized CBM2 acceleration do not by themselves establish that execution model.

## Fixed product boundary

- Cassie remains single-node, with Midge as the only direct storage layer and the
  owner of durability/recovery mechanics; Cassie owns query-visible semantics.
- PostgreSQL wire remains the primary query interface. REST remains secondary and
  administrative. This target does not create an alternate query/storage service.
- The accepted on-disk layout remains cassie-midge-layout-v2. No legacy reader,
  migration or compatibility support is introduced by this target.
- PostgreSQL 18 supplies a version-pinned semantic/protocol comparison, not a new
  server/client certification. Only selected documented workflows are claimed.
- Full PostgreSQL syntax/server/catalog/extension/isolation parity, pgvector ABI,
  trigger/business-procedure platforms, distributed SQL and fleet coordination
  remain unclaimed. Remote provider availability and quotas remain operator-owned.

## Finite capability matrix

Current labels below summarize the canonical matrix; they are not a second status
authority. Planned rows name absent capabilities accepted into the backlog.
Correct fallback preserves current semantics but does not count as a native typed
kernel. The complete native-kernel/type matrix is pinned by [#752](https://github.com/cntryl/cassie/issues/752)/[#753](https://github.com/cntryl/cassie/issues/753) before adoption.

| Capability | Current finite surface / status | Target work and explicit boundary | Owner / qualification | Oracle / promotion gate |
| --- | --- | --- | --- | --- |
| Core SQL reads, predicates, CASE and COALESCE | Stable core reads/predicates; exact documented NULL/type/error semantics. | Qualify the finite contextual Boolean contract; retain name resolution, excluded-lane evaluation and errors. No ordinary AND/OR evaluation-order promise. | [#432](https://github.com/cntryl/cassie/issues/432); [#756](https://github.com/cntryl/cassie/issues/756); [#761](https://github.com/cntryl/cassie/issues/761) | Boolean/type laws and authoritative scalar results; complete selected differential cases before any support review. |
| Ordering, pagination, deduplication and sets | Stable ORDER BY/NULL placement/LIMIT/OFFSET, DISTINCT/DISTINCT ON, UNION/UNION ALL/INTERSECT/EXCEPT. | Typed ordering/set kernels retain multiplicities, row identity and exact integer/ARRAY comparisons. | [#760](https://github.com/cntryl/cassie/issues/760); [#761](https://github.com/cntryl/cassie/issues/761) | Scalar comparison/equality/hash and bags or declared order; bounded blocking state and controlled failure. |
| Aggregation | Stable COUNT/SUM/AVG/MIN/MAX, GROUP BY/HAVING. | Native typed/encoded subset; preserve COUNT BIGINT, empty/all-NULL laws, checked integer overflow and defined FLOAT row-order fold or fallback. | [#758](https://github.com/cntryl/cassie/issues/758); [#761](https://github.com/cntryl/cassie/issues/761) | Exact scalar/hand-derived identities and overflow fixtures; no silent FLOAT reassociation. |
| Joins | Experimental documented inner/outer/cross/lateral/apply/semi/anti forms. | Finite typed build/probe subset, exact NULL/duplicate/outer/residual behavior and legal reorder/switch fallback. | [#759](https://github.com/cntryl/cassie/issues/759); [#761](https://github.com/cntryl/cassie/issues/761) | Legal scalar/indexed/merge joins; reserve build, hot-key fanout and output before retention. |
| Table/predicate/lateral/correlated subqueries and CTEs | Experimental documented forms, including EXISTS and nonrecursive/recursive WITH. | Qualify only implemented grammar/scope/recursive forms; deterministic unsupported errors. | [#761](https://github.com/cntryl/cassie/issues/761) | Exact supported semantics, types/NULLs/errors and empty/recursive boundary cases. |
| Scalar expression subqueries | Planned; SELECT (SELECT 1) fails 42601 at the baseline. | Selected zero/one/many-row, width/type/correlation/expression/resource contract precedes bounded implementation. | [#754](https://github.com/cntryl/cassie/issues/754) documentation; [#762](https://github.com/cntryl/cassie/issues/762) implementation after [#761](https://github.com/cntryl/cassie/issues/761) | Independent cardinality/scope fixtures and Describe/Execute metadata; no claim while absent. |
| Windows | Experimental ranking/offset/value functions and documented row frames. | Finite typed/frame subset with exact stage/peer/order/NULL semantics and bounded state. | [#760](https://github.com/cntryl/cassie/issues/760); [#761](https://github.com/cntryl/cassie/issues/761) | Existing scalar window/frame fixtures; retain explicit frame/syntax exclusions. |
| Logical types and codecs | Experimental types/casts; current declared universe listed below. | Shared logical/physical/codec identity, exact known deviations and selected kernel capability authority. | [#752](https://github.com/cntryl/cassie/issues/752); [#753](https://github.com/cntryl/cassie/issues/753); [#776](https://github.com/cntryl/cassie/issues/776) | Exact text/binary/metadata fixtures, large integer and malformed-input boundaries. No new persistent type/ABI. |
| Physical ColumnStore | Experimental field-key tables; separate from Midge families and CBM2 sidecars. | Controlled selected-field reconstruction, row/field atomicity, schema/lifecycle and typed-reader qualification. | [#749](https://github.com/cntryl/cassie/issues/749); [#756](https://github.com/cntryl/cassie/issues/756); [#768](https://github.com/cntryl/cassie/issues/768) | Authoritative rows plus pre-decode reservations, corruption/generation and restart cases. No broad column-store promotion here. |
| CBM2 column-batch acceleration | Stable current plain/constant/RLE/dictionary/frame-of-reference/ALP/FSST codecs, pruning, selected projection and aggregate paths. | Preserve current latest-only framing/generation/fallback rules; hand off eligible typed views without forced JSON conversion. | [#756](https://github.com/cntryl/cassie/issues/756); [#758](https://github.com/cntryl/cassie/issues/758); [#768](https://github.com/cntryl/cassie/issues/768) | Existing golden codecs, complete validation and authoritative row comparison; existing paired codec benchmark objectives. |
| Common typed-column execution | Planned; current Batch is a vector of named scalar BatchRow values. | One private batch/view contract; scan/filter/projection, then aggregates/joins, then ordering/sets/windows. Every current supported logical value must survive declared output/fallback boundaries. | [#753](https://github.com/cntryl/cassie/issues/753); [#756](https://github.com/cntryl/cassie/issues/756); [#758](https://github.com/cntryl/cassie/issues/758)–[#760](https://github.com/cntryl/cassie/issues/760) | Constructor/ownership laws, scalar results and selected type/kernel matrix. No public API or disk format chosen here. |
| Portable kernels and optional SIMD | Planned for the common relational target; vector-distance SIMD already exists independently. | Portable selected kernels are required; SIMD only for a measured beneficial eligible kernel with feature/alignment/tail/mask rules. | [#774](https://github.com/cntryl/cassie/issues/774) | Portable exact kernel plus dispatch/tail/NULL cases and measured performance; no required SIMD adoption. |
| Pull controls, workers, caches and adaptive paths | Experimental broad surfaces; selected opt-in adaptive scalar/join switching is Stable. | Share engine limits/context; accurate final typed/row/fallback diagnostics; retain opt-in scope and deterministic merges. | [#749](https://github.com/cntryl/cassie/issues/749); [#766](https://github.com/cntryl/cassie/issues/766); [#778](https://github.com/cntryl/cassie/issues/778) | Controlled failure/cleanup and fresh semantic execution; disk spilling remains absent. |
| Pgwire framing and messages | Experimental documented PostgreSQL v3 startup, password/transport, simple/extended messages, formats, COPY and cancellation. | Finite packet/state matrix and exact workflow evidence; advanced negotiation/auth/replication are not implied. | [#755](https://github.com/cntryl/cassie/issues/755); [#757](https://github.com/cntryl/cassie/issues/757); [#780](https://github.com/cntryl/cassie/issues/780) | Version-pinned state tables and secret-free exact-revision transcripts, including malformed/recovery boundaries. |
| Wire-cycle transactions and portals | Experimental explicit transactions/savepoints/read-your-writes; simple implicit-batch difference reproduced. | The bounded Query/Sync profile is user selected; state-table implementation, extended/unnamed and extended-to-simple interleaving probes remain pending. | [#755](https://github.com/cntryl/cassie/issues/755); [#757](https://github.com/cntryl/cassie/issues/757); [#763](https://github.com/cntryl/cassie/issues/763) | Independent-session state and I/T/E/frame counts; standalone DDL/mixed-cycle exclusions, no transactional-DDL or stronger-isolation promise. |
| DML, constraints and document identity | Experimental DML/table/constraint surface; reserved _id behavior is Stable. | Preserve statement atomicity, constraints/defaults/FK/COPY and declared id versus internal identity across ingress. | [#432](https://github.com/cntryl/cassie/issues/432); [#763](https://github.com/cntryl/cassie/issues/763); [#769](https://github.com/cntryl/cassie/issues/769) | Authoritative pre/post state and exact results/errors; no blanket general OLTP claim. |
| Database/schema/session/catalog scope | Stable database/schema/search_path/CONNECT plus named information_schema tables/columns; remaining catalogs Experimental. | Finite supported columns/rows/settings and live authorization/lifecycle per selected clients. | [#766](https://github.com/cntryl/cassie/issues/766); [#777](https://github.com/cntryl/cassie/issues/777) | Current catalog contract and exact client probes; full internal catalog parity remains unclaimed. |
| Views, local projections and rollups | Views and documented local projection lifecycle Stable; rollups/retention Experimental. | Verify existing substitution and publication contracts across typed handoffs; no remote replay or automatic activation expansion. | [#773](https://github.com/cntryl/cassie/issues/773) | Original source query, durable ledger/generation state and finite interruption/corruption barriers. |
| Narrow procedures and scalar UDFs | One-statement procedure/CALL subset Stable; scalar UDFs Experimental. | Qualify only documented body/volatility/schema/client workflows. | [#761](https://github.com/cntryl/cassie/issues/761); [#777](https://github.com/cntryl/cassie/issues/777) | Selected SQL/metadata tests; PL/pgSQL, triggers, recursion/transaction-control and business workflows excluded. |
| Exact embedding-vector retrieval | Stable finite f32 VECTOR(n), cosine/dot/L2 and authoritative top-k. | Pin zero-vector, dimension/finite/type/order/offset and interface equivalence boundaries. | [#750](https://github.com/cntryl/cassie/issues/750); [#776](https://github.com/cntryl/cassie/issues/776) | Independent exact metrics and deterministic row-id ranking; no halfvec/sparsevec/bit/operator-class or pgvector ABI promise. |
| HNSW and IVFFlat | Stable declared candidate selection and authoritative reranking; approximate recall has its own contract. | Retain completed candidate-exhaustion/resource repairs; qualify generation/framing, overlays and recoverable publication. | [#748](https://github.com/cntryl/cassie/issues/748); [#764](https://github.com/cntryl/cassie/issues/764) | Exact candidate distances/cardinality plus retained parameterized recall >=0.90 artifacts. Reranking does not prove globally exact ANN top-k. |
| Full-text and hybrid retrieval | Stable documented analyzers, exact BM25 and declared candidate/scoring paths. | Pin hybrid approximation/visibility; probe corpus stats and final source-generation seams before asserting races. | [#765](https://github.com/cntryl/cassie/issues/765) | Whole-corpus stats, declared formula, authoritative fallback and deterministic generation barriers; no implicit globally exact hybrid promise. |
| Embedding providers and REST retrieval | Local deterministic hashing Stable; remote providers and REST Experimental. | Budget/cap cleanup, identity/configuration, timeout/retry/body/cardinality and generation/cache probes. | [#750](https://github.com/cntryl/cassie/issues/750); [#767](https://github.com/cntryl/cassie/issues/767); [#770](https://github.com/cntryl/cassie/issues/770) | Local/mock response fixtures and uncached source/control counters; mocked coverage is not third-party availability. |
| Graph/time-series/retention overlaps | Graph Experimental; fixed positive integer minute/hour/day UTC indexes Stable; rollup/retention Experimental. | Preserve exact current topology/range semantics; unsupported second/week/month/calendar/local/DST widths reject time-series index DDL, supported stale/corrupt artifacts fall back atomically. | [#754](https://github.com/cntryl/cassie/issues/754) documentation; [#772](https://github.com/cntryl/cassie/issues/772); [#775](https://github.com/cntryl/cassie/issues/775) | Hand-derived graph/UTC boundaries, authoritative rows, retention side effects and controlled failure. |
| Storage, recovery and images | Current layout/Midge ownership and declared derived/publication/image contracts. | Qualify database ownership/schema lifecycle, restart/publication/images and trusted backup/restore procedures. | [#771](https://github.com/cntryl/cassie/issues/771) | Authoritative recovered state and existing journal/image/checksum/failure contracts; no parallel WAL/recovery engine. |
| Native operational promotion | Production Candidate; representative complete native evidence and objectives remain open. | Exact revision/digest/profile, repeated benchmarks, recovery/scale/soak and local diagnostic promotion. | [#8](https://github.com/cntryl/cassie/issues/8); [#29](https://github.com/cntryl/cassie/issues/29); [#781](https://github.com/cntryl/cassie/issues/781) | Named native Linux amd64/arm64 bundles; diagnostic/smoke runs and green issue lists cannot certify readiness. |

## Current finite logical type universe

The metadata carrier Null is distinguished from declared SQL types. Current
logical types are SMALLINT, INT, BIGINT, FLOAT/DOUBLE (float8; NUMERIC/DECIMAL are
aliases with documented loss of exact-decimal behavior), BOOLEAN, TEXT, CHAR(n),
VARCHAR(n), UUID, BYTEA, DATE, TIME, one normalized TIMESTAMP type, JSON, VECTOR(n),
and one-dimensional ARRAY of a permitted non-ARRAY element. Current bounds and
wire identities are derived from src/types/schema.rs, not imported from an
extension. The [finite SQL type and wire contract](type-contract.md) records the
selected input/output/type-name/OID/typlen/typmod and storage representations
under [#752](https://github.com/cntryl/cassie/issues/752), before
[#753](https://github.com/cntryl/cassie/issues/753) selects private lane representations.

Preserve exact integer comparison/hash behavior, declared temporal units, SQL NULL
versus JSON null, finite f32 vector dimensions and current array bounds/private
OIDs. Exact NUMERIC, separate TIMESTAMPTZ, padded CHAR, multidimensional/custom-bound
arrays and pgvector types/OIDs are not newly selected capabilities. A new type,
public identity, durable encoding or migration requires a concrete contract first.

## Selected client/workflow boundary

The matrix adopts the already documented named workflows, not every operation a
client can generate. Exact versions are in [Compatibility Probe Contract](compatibility-probe-contract.md).

| Client lane | Finite workflow | Evidence classification / remaining owner |
| --- | --- | --- |
| Native pgwire / tokio-postgres harness | Connection, selected metadata, simple/extended parameters/formats, bounded transactions/COPY/cancellation. | Existing repository tests are mapped; [#755](https://github.com/cntryl/cassie/issues/755)/[#757](https://github.com/cntryl/cassie/issues/757)/[#780](https://github.com/cntryl/cassie/issues/780) retain fresh exact-revision protocol acceptance. |
| sqlx 0.8.3 | Documented catalog discovery, prepared parameters, committed write, rollback and deterministic errors. | Existing retained named-workflow evidence; [#777](https://github.com/cntryl/cassie/issues/777) owns selected fresh client review, not migrations/macros. |
| Diesel 2.2.6 | Documented catalog discovery, bound SQL, committed write, rollback and deterministic errors. | Existing retained named-workflow evidence; typed DSL/migrations/schema generation are outside the claim. |
| Prisma 6.1.0, SQLAlchemy 2.0.36 with psycopg 3.2.3, psql 16.6 | Existing opt-in read-model probe workflows only. | Lane existence is not retained certification; [#777](https://github.com/cntryl/cassie/issues/777) records actual passed/failed/unavailable exact-revision evidence. |
| pgAdmin 9.16 | Planned password/TLS/reconnect, supported navigator/properties, Query Tool/results/transactions/cancel/plans/safe key-based grid edit. | Normalized trace replay is prerequisite only; live desktop gate/artifacts unavailable and unverified. [#777](https://github.com/cntryl/cassie/issues/777). |
| DBeaver 26.1.3 / PostgreSQL JDBC 42.7.11 | The same selected supported desktop workflow envelope. | Normalized trace replay is prerequisite only; live desktop gate/artifacts unavailable and unverified. [#777](https://github.com/cntryl/cassie/issues/777). |
| Other/new client versions | No certification inherited from the above lanes. | Select finite workflows and retain new exact-version evidence before support claims. |

No new self-hosted runner work is part of this target. Repository harness,
trace-replay, opt-in external-driver and actual desktop evidence remain separate.
GUI DDL dialogs, maintenance/backup tools, replication, extensions and internal
catalog parity remain outside the current named desktop target.

## Completion and promotion gates

1. Contracts/correctness: close each P0 reproduction with red/green evidence;
   publish [#751](https://github.com/cntryl/cassie/issues/751)/[#754](https://github.com/cntryl/cassie/issues/754) truth, [#752](https://github.com/cntryl/cassie/issues/752) type identity and [#753](https://github.com/cntryl/cassie/issues/753) private batch laws;
   document and qualify [#755](https://github.com/cntryl/cassie/issues/755)'s user-selected bounded transaction profile before
   treating its runtime dependency as complete.
2. Typed batches/streaming: [#756](https://github.com/cntryl/cassie/issues/756) implements scan/filter/projection only after
   [#753](https://github.com/cntryl/cassie/issues/753)/[#749](https://github.com/cntryl/cassie/issues/749)/[#432](https://github.com/cntryl/cassie/issues/432)/[#755](https://github.com/cntryl/cassie/issues/755); [#757](https://github.com/cntryl/cassie/issues/757) qualifies selected portal ownership after [#755](https://github.com/cntryl/cassie/issues/755)/[#752](https://github.com/cntryl/cassie/issues/752).
3. Relational kernels: [#758](https://github.com/cntryl/cassie/issues/758) aggregates and [#759](https://github.com/cntryl/cassie/issues/759) joins follow [#756](https://github.com/cntryl/cassie/issues/756); [#760](https://github.com/cntryl/cassie/issues/760)
   ordering/sets/windows follows both; [#761](https://github.com/cntryl/cassie/issues/761) qualifies completed row/native paths;
   [#762](https://github.com/cntryl/cassie/issues/762) adds bounded scalar expression subqueries only after [#761](https://github.com/cntryl/cassie/issues/761).
4. Storage/retrieval composition: complete the dependent [#763](https://github.com/cntryl/cassie/issues/763)–[#776](https://github.com/cntryl/cassie/issues/776) owners listed
   in the ledger. Existing formats and support contracts remain authoritative;
   unproven race/corruption seams get deterministic probes, not defect labels.
5. Compatibility/release: [#777](https://github.com/cntryl/cassie/issues/777)/[#778](https://github.com/cntryl/cassie/issues/778)/[#779](https://github.com/cntryl/cassie/issues/779)/[#780](https://github.com/cntryl/cassie/issues/780) qualify selected clients, controls,
   cross-path laws and wire packets; [#8](https://github.com/cntryl/cassie/issues/8) then [#29](https://github.com/cntryl/cassie/issues/29) retain native threshold/local
   diagnostics promotion; [#781](https://github.com/cntryl/cassie/issues/781) consolidates one immutable release ledger.

For an invariant, completion requires executed selected acceptance or an explicitly
approved exclusion. A linked unresolved blocker records remaining work and its next
action; it remains open and prevents claiming that acceptance complete. For a feature promotion, require implementation,
documented finite behavior, independent semantic/failure/resource evidence and
explicit feature-owner review in Feature Support. For Production-ready, also
require every native readiness/operational blocker at one immutable revision.
Neither an issue closure nor a mapped test changes support status automatically.

Reuse [Query Promotion Evidence](query-promotion-evidence.md) and existing focused
fixtures; add only concrete missing probes. Compare bags unless SQL orders rows;
compare types, NULLs, errors, visibility, side effects and final path diagnostics.
ANN recall is a separate measured contract and cannot serve as an exact relational
oracle. Memory evidence means accounted retained query memory, not process RSS.

Named native profiles and artifact identities are already defined in
[Deployment Profiles](deployment-profiles.md), [Performance Contracts](performance-contracts.md)
and [Production Readiness](production-readiness.md). [#8](https://github.com/cntryl/cassie/issues/8) owns repeated native-disk
windows/objectives/variance/recovery/endurance; [#29](https://github.com/cntryl/cassie/issues/29) already has local diagnostic
field contracts and retains remaining native promotion; [#781](https://github.com/cntryl/cassie/issues/781) may not duplicate or
bypass those owners. Do not invent numeric SLOs, release dates or new runners.

## Selected contracts and outstanding decisions

| Decision | Owner / next concrete artifact | Boundary |
| --- | --- | --- |
| Exact shared logical/codec and private native-kernel/type matrix | [#752](https://github.com/cntryl/cassie/issues/752) type table, then [#753](https://github.com/cntryl/cassie/issues/753) constructor/view/ownership laws and [#756](https://github.com/cntryl/cassie/issues/756) coverage matrix. | Current finite type universe only; private in-memory representation is not inherently a public API. |
| Implicit Query/Sync visibility, DDL/COPY exceptions and portal lifetime | [#755](https://github.com/cntryl/cassie/issues/755) state tables and independent-session/unnamed/extended-to-simple probes for the user-selected bounded profile. | Selected by [the durable direction](https://github.com/cntryl/cassie/issues/755#issuecomment-5982444561); implementation and qualification remain pending. |
| Hybrid approximation and permitted source generation | [#765](https://github.com/cntryl/cassie/issues/765) candidate/scoring contract and deterministic corpus/source barriers. | Pending probe/finite specification; no globally exact hybrid claim. |
| Any expanded type/OID/extension, durable layout/recovery encoding or public API | Owning issue must prepare a concrete reviewable contract before implementation. | Not selected by this foundation document. |
| Native objectives and promotion | [#8](https://github.com/cntryl/cassie/issues/8) retained comparable repetitions, then [#29](https://github.com/cntryl/cassie/issues/29) and [#781](https://github.com/cntryl/cassie/issues/781) reviews. | Existing advisory values remain advisory; no global readiness promotion here. |

The selected [#755](https://github.com/cntryl/cassie/issues/755) profile commits eligible simple read/DML batches at successful
Query completion and eligible extended read/DML cycles at successful Sync; an
error rolls back the implicit segment/cycle. Explicit transactions retain their
state across Sync. Preserve explicit table COPY staging, keep DDL standalone and
reject unsupported mixed cycles. A completed standalone DDL is not rolled back
by a later streamed command. Portal lifetime and interleaving still need the
selected state tables and independent-session protocol evidence. The selection
does not add transactional DDL, stronger isolation or database-image transaction
semantics, close [#755](https://github.com/cntryl/cassie/issues/755), or qualify its current runtime.
