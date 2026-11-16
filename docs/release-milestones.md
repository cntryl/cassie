# Release Milestones

This page maps the live open issue backlog to the v0.1.0 and v0.2.0 releases and
the later v0.3.0 (composite types and JSON recordsets), v0.4.0 (named enums) and
v0.5.0 (authoritative columnar tables) and v0.6.0 (graph queries) milestones. The [Product Roadmap](product-roadmap.md)
owns release purpose and scope. GitHub milestone membership and native issue
dependencies are the live delivery record.

## v0.1.0 Projection Store

This is Cassie's actual unreleased starting point. It establishes durable
single-node projection writes, replay recovery, schema/object lifecycle,
rebuild, and the selected PostgreSQL-wire and named-client workflows. The
former v0.2.0 package value and the former v0.3 Projection Store/v0.4 Query
planning labels are not prior releases or compatibility commitments. The exact
v0.1.0 release commit still requires its own local gate. Baseline/recovery PR
[#1144](https://github.com/cntryl/cassie/pull/1144) merged as
`b9afc913fd1e65d1888df922f1d33e280f08f611` on 2026-10-30. The user directed
that Actions should be enabled on 2026-11-04; normal push and PR checks are
authorized under the updated `AGENTS.md`.

The GitHub v0.1.0 milestone was closed on 2026-11-04 after live verification
of zero open issues and 13 closed issues. Release publication and exact-commit
qualification remain separately evidenced.

## v0.2.0 Query and Retrieval

This release adds the finite read-side SQL and retrieval feature on top of the
0.1.0 projection store. Scope is grouped by observable behavior:

- Relational expressions and predicates: #762, #796, #798, #799, #800.
- Arrays, table functions, JSON, and collection aggregates: #807–#811 (#829's
  recordset work moved to v0.3.0).
- Aggregate/window behavior and numeric/time functions: #805, #806, #813,
  #815.
- Type and catalog support: moved to v0.3.0 on 2026-11-12 (#817, #818, #828,
  #829).
- Transaction snapshots, table DDL, authorization, and schema evolution:
  #819, #831, #833, #839 (#826 moved to v0.5.0).
- Row-authoritative index behavior and object comments: #842, #845.
- Bounded extension and retrieval compatibility: #843, #844.
- Synchronized cancellation correctness probes for query and retrieval paths:
  #954.

### Stacked delivery bundles selected 2026-11-04 (historical; #817, #818, #826, #828 and #829 have since moved to v0.3.0/v0.5.0)

The user requested a handful of related feature PRs, stacked without waiting for
parent review completion, with independent work in isolated worktrees. A
contract/process foundation PR precedes these five logical bundles:

| Bundle | Issues | Dependency and acceptance boundary |
| --- | --- | --- |
| Boolean and scalar functions | #805, #813, #815 | Existing aggregate/math/time prerequisites are closed. Qualify current implementations and repair confirmed defects, including execution-boundary cancellation. |
| Query expressions and arrays | #762, #796, #798, #799, #800, #807, #808 | Resolve casts/subqueries before dependent arrays and set functions. Persisted null-safe definitions use the current-format contract; no pre-release migration or old-reader compatibility is required. |
| Row lifecycle, snapshots and type catalogs | #817, #818, #819, #826, #828, #833, #839, #842 | Complete selected catalog publication before dependent table/type evolution; casts precede object pseudo-types. Verify direct Midge retained-view support before snapshot runtime. |
| JSON, collection/statistical functions and cancellation | #806, #809, #810, #811, #829, #954 | JSON operators precede construction; arrays, table functions and named records precede dependent collections/recordsets. Reconcile all named cancellation fixtures against current source. |
| Reader/projector access and extension retrieval | #831, #843, #844, #845 | Catalog identities precede grants/comments/extensions; extension identity precedes the selected vector descriptor/retrieval profile. |

The table records logical ownership, not completed acceptance or a limit on
necessary review splits. Agents may prepare independent increments in parallel;
combine them into their coherent bundle before its final gate. Children target
parent branches, merges follow prerequisite order, and rebases require affected
validation and review again on the final head. Do not close an issue from an
agent report, authored test, Jev probability, or a partial implementation.

Every issue must retain a finite Cassie contract and the named read-model or
client workflow that requires it. Full PostgreSQL parity is out of scope.
In particular, #826 and #842 cover row-authoritative tables; column authority
and its derived indexes belong to v0.5.0.

## v0.2.0 narrowing (2026-11-12)

v0.2.0 is now "stabilize the delivered feature set and make it defect-free";
new features moved to later milestones. On 2026-11-12 #817, #818, #828 and #829
moved to v0.3.0 and #826 moved to v0.5.0. Forms not delivered under #810
(`row_to_json`/`array_to_json`, `IS JSON`, `JSON_VALUE`, `JSON_QUERY`,
`PASSING`; explicit `VARIADIC text[]` for `json*_extract_path` is implemented) and the #844 pgvector
function-name surface are deferred and are not v0.2.0 acceptance criteria.

## v0.3.0 Composite Types and JSON Recordsets

Milestone 12 held #817 (catalog introspection), #818 (object identifiers), #828
(composite and domain types) and #829 (JSON recordsets). The GitHub milestone
is closed with no open issues. This does not establish composite-array support
for graph paths, release publication, or exact-head qualification.

## v0.6.0 Fast, Idiomatic Graphs

The [graph milestone](https://github.com/cntryl/cassie/milestone/13) targets
property graph queries over projector-maintained tables through PostgreSQL
clients. Its [contract](graph-v060-contract.md) owns the semantics,
building blocks, dependencies and qualification criteria, and the
[graph guide](graph-guide.md) describes the delivered surface. Mapped property graphs,
`GRAPH_TABLE`, the `cassie_graph` path types and `graph_weighted_paths` are implemented on
exact reads, and the legacy graph feature is removed. Implemented behavior is not
qualification: the [promotion evidence](graph-v060-gap-analysis.md#promotion-evidence)
lists what is proven and what remains open, and graph support stays Experimental until it
is decided from that evidence. Earlier documented release dependencies remain in force.

Delivery order, by owner decision of 2026-11-15: mapped graphs (#1255), native
path types (#1268), the `GRAPH_TABLE` surface (#1256), exact execution (#1257
and the transaction-view part of #1254), weighted paths and removal of the
legacy graph feature without conversion (#1258), fast reads (#1253, the
read-bound part of #1254, and #1259), then qualification (#1260). Query
execution uses exact reads of the mapped tables until the fast-reads step.

## Replay durability and throughput

The [replay durability and throughput milestone](https://github.com/cntryl/cassie/milestone/14)
tracks buffered replay durability and workload evidence
([#1271](https://github.com/cntryl/cassie/issues/1271)), alongside the public
checkpoint-view snapshot work ([#1272](https://github.com/cntryl/cassie/issues/1272)).
The atomic replay prerequisite [#1270](https://github.com/cntryl/cassie/issues/1270)
is closed. The replay API policy slice [#1276](https://github.com/cntryl/cassie/issues/1276)
does not complete the SQL projector mode, cloud qualification, or the broader
performance milestone. Statement-level co-snapshot semantics for the public view
are not yet claimed.

## Deferred catalog compatibility

The selected extension-membership rows in `pg_depend` are absent and are
tracked by [#1273](https://github.com/cntryl/cassie/issues/1273). A previously
unregistered test requiring those rows is not evidence of existing support;
the catalog feature remains outside the replay milestone.

## v0.5.0 Authoritative Columnar Tables

This is a substantial storage feature, not a relabeling of existing derived
column-batch indexes. #1105 defines the contract; #1106 adds table-mode DDL and
metadata; #1107 implements authoritative storage and recovery; #1108 adds
atomic mixed-mode writes; #1109 covers mixed-mode queries; #1110 covers
constraints; #1111 covers derived indexes; and #1112 covers conversion and
database images. Midge remains the only storage layer. The GitHub milestone is
closed with no open issues; see [qualification](v0.5-release-qualification.md)
for evidence and remaining release gates. Its historical native blocker edges
preserved the stated order: #1106 follows #1105; #1107
follows #1105/#1106; #1108 follows #1105–#1107; #1109 follows #1105–#1108;
#1110 follows #1105–#1107; #1111 follows #1105/#1107; and #1112 follows
#1105–#1108. The row-authoritative DDL issue #826 also waits on #833; schema
evolution #839 already waits on #833.

## v0.4.0 Named Enum Types

This milestone contained #827 and is closed with no open issues. Its acceptance scope is named ENUM
creation, lookup, alteration and drop, plus literal, parameter, cast,
comparison, array and result identities. The milestone
does not imply general PostgreSQL type or catalog parity; its target date is
TBD.

## Reworked and closed outside the release plan

These issues were retained as history, retitled as disposition records, and
closed as not planned. Their capabilities are absent and are not regressions.
Reopen a capability only when a concrete read-model workflow and finite
acceptance contract justify it.

| Issue | Disposition |
| --- | --- |
| #820 | Row locks and `SKIP LOCKED` claims describe a work-queue feature; Cassie's product contract does not select queue ownership. |
| #821 | The selected prepared-query, transaction, search-path, catalog, and error-recovery client workflows are complete. Broader settings, cursors, and reset behavior have no remaining named workflow. |
| #822 | Advisory locks and `LISTEN/NOTIFY` are not required by the selected projection, query, or retrieval workflows. |
| #823 | The selected projection DML and rebuild path is covered by v0.1.0. `MERGE` and joined mutation forms lack a named read-model workflow. |
| #838 | Temporary relations and session-local lifecycle have no selected migration or application workflow. |
| #846 | The selected COPY rebuild ingress and documented limited COPY output are complete; generic dump/restore and extra COPY forms have no selected workflow. |
| #905 | Range and multirange types have no selected read-model use case or active type contract. |
| #906 | XML values and SQL/XML functions have no selected read-model use case or active type contract. |
| #907 | Geometric types and functions have no selected read-model use case or active type contract. |
| #908 | Money values and arithmetic have no selected read-model use case or active type contract. |
| #959 | Closed as not planned on 2026-10-30. CI deadline tuning requires a workflow change, which is not authorized by the current repository direction. |

## Production qualification

Production readiness remains a separate evidence gate described in
[Production Readiness](production-readiness.md) and the release checklist. It
covers representative-scale behavior, concurrency, recovery, operations, and
release evidence. It is not a fourth feature milestone. The former GitHub
milestone #9 is closed; no v1.0 feature milestone remains open.
