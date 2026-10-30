# Release Milestones

This page maps the live open issue backlog to the three planned feature
releases. The [Product Roadmap](product-roadmap.md) owns release purpose and
scope. GitHub milestone membership and native issue dependencies are the live
delivery record.

## v0.1.0 Projection Store

This is Cassie's actual unreleased starting point. It establishes durable
single-node projection writes, replay recovery, schema/object lifecycle,
rebuild, and the selected PostgreSQL-wire and named-client workflows. The
former v0.2.0 package value and the former v0.3 Projection Store/v0.4 Query
planning labels are not prior releases or compatibility commitments. The exact
v0.1.0 release commit still requires its own local gate; Actions remain
disabled. The active baseline/recovery PR
[#1144](https://github.com/cntryl/cassie/pull/1144) is assigned to this
milestone.

## v0.2.0 Query and Retrieval

This release adds the finite read-side SQL and retrieval feature on top of the
0.1.0 projection store. Scope is grouped by observable behavior:

- Relational expressions and predicates: #762, #796, #798, #799, #800.
- Arrays, table functions, JSON, and collection aggregates: #807–#811.
- Aggregate/window behavior and numeric/time functions: #805, #806, #813,
  #815.
- Type and catalog support: #817, #818, #827, #828, #829.
- Transaction snapshots, table DDL, authorization, and schema evolution:
  #819, #826, #831, #833, #839.
- Row-authoritative index behavior and object comments: #842, #845.
- Bounded extension and retrieval compatibility: #843, #844.
- Synchronized cancellation correctness probes for query and retrieval paths:
  #954.

Every issue must retain a finite Cassie contract and the named read-model or
client workflow that requires it. Full PostgreSQL parity is out of scope.
In particular, #826 and #842 cover row-authoritative tables; column authority
and its derived indexes belong to v0.3.0.

## v0.3.0 Authoritative Columnar Tables

This is a substantial storage feature, not a relabeling of existing derived
column-batch indexes. #1105 defines the contract; #1106 adds table-mode DDL and
metadata; #1107 implements authoritative storage and recovery; #1108 adds
atomic mixed-mode writes; #1109 covers mixed-mode queries; #1110 covers
constraints; #1111 covers derived indexes; and #1112 covers conversion and
database images. Midge remains the only storage layer. Implementation waits
for the persistent format, transaction, SQL, and migration contract in #1105.
Native blocker edges preserve the stated order: #1106 follows #1105; #1107
follows #1105/#1106; #1108 follows #1105–#1107; #1109 follows #1105–#1108;
#1110 follows #1105–#1107; #1111 follows #1105/#1107; and #1112 follows
#1105–#1108. The row-authoritative DDL issue #826 also waits on #833; schema
evolution #839 already waits on #833.

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
| #959 | CI deadline tuning requires workflow changes while Actions are disabled; no workflow change is authorized by the current release scope. |

## Production qualification

Production readiness remains a separate evidence gate described in
[Production Readiness](production-readiness.md) and the release checklist. It
covers representative-scale behavior, concurrency, recovery, operations, and
release evidence. It is not a fourth feature milestone. The former GitHub
milestone #9 is closed; no v1.0 feature milestone remains open.
