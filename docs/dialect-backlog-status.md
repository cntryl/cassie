# Generic SQL dialect backlog status

This is a point-in-time tracker refresh for [#791](https://github.com/cntryl/cassie/issues/791), captured 2026-10-10 12:11 UTC. It records live GitHub issue and PR state; it is not implementation, conformance, feature-support, or production-readiness evidence. Issue state alone does not prove runtime behavior.

## Child-owner inventory

The 57 planned child owners numbered #793–#849 are **53 open** and **4 closed** in the captured GitHub snapshot. Closed means the owning issue is closed; it does not imply every broader dialect cell or readiness gate is complete.

| Issue | Current title | State |
| --- | --- | --- |
| [#793](https://github.com/cntryl/cassie/issues/793) | Specify extended SQL type, wire and storage contracts | Open |
| [#794](https://github.com/cntryl/cassie/issues/794) | Tokenize PostgreSQL operators independently of whitespace | Closed |
| [#795](https://github.com/cntryl/cassie/issues/795) | Bind ordinary relation aliases and qualified references | Closed |
| [#796](https://github.com/cntryl/cassie/issues/796) | Complete PostgreSQL postfix cast and type-name syntax | Open |
| [#797](https://github.com/cntryl/cassie/issues/797) | Bind parameters and expressions in LIMIT and OFFSET | Closed |
| [#798](https://github.com/cntryl/cassie/issues/798) | Implement null-safe comparison predicates | Open |
| [#799](https://github.com/cntryl/cassie/issues/799) | Add ILIKE and PostgreSQL regular-expression predicates | Open |
| [#800](https://github.com/cntryl/cassie/issues/800) | Support VALUES relations and row-value expressions | Open |
| [#801](https://github.com/cntryl/cassie/issues/801) | Support explicit CTE materialization modifiers | Open |
| [#802](https://github.com/cntryl/cassie/issues/802) | Add NULLIF, GREATEST and LEAST expressions | Closed |
| [#803](https://github.com/cntryl/cassie/issues/803) | Extend PostgreSQL string functions and formatting syntax | Open |
| [#804](https://github.com/cntryl/cassie/issues/804) | Support aggregate DISTINCT, FILTER and ordered arguments | Open |
| [#805](https://github.com/cntryl/cassie/issues/805) | Add BOOL_AND and BOOL_OR aggregates | Open |
| [#806](https://github.com/cntryl/cassie/issues/806) | Add statistical and ordered-set aggregates | Open |
| [#807](https://github.com/cntryl/cassie/issues/807) | Add PostgreSQL array constructors, predicates and indexing | Open |
| [#808](https://github.com/cntryl/cassie/issues/808) | Execute UNNEST and table-function ordinality | Open |
| [#809](https://github.com/cntryl/cassie/issues/809) | Implement PostgreSQL JSON operators and path predicates | Open |
| [#810](https://github.com/cntryl/cassie/issues/810) | Add JSON inspection, construction and conversion functions | Open |
| [#811](https://github.com/cntryl/cassie/issues/811) | Add ordered string, array and JSON collection aggregates | Open |
| [#812](https://github.com/cntryl/cassie/issues/812) | Implement exact NUMERIC and DECIMAL values | Open |
| [#813](https://github.com/cntryl/cassie/issues/813) | Add typed rounding and numeric scalar functions | Open |
| [#814](https://github.com/cntryl/cassie/issues/814) | Implement TIMESTAMPTZ and calendar INTERVAL semantics | Open |
| [#815](https://github.com/cntryl/cassie/issues/815) | Implement SQL clock and calendar functions | Open |
| [#816](https://github.com/cntryl/cassie/issues/816) | Add bit-string, bytea and hashing conversion functions | Open |
| [#817](https://github.com/cntryl/cassie/issues/817) | Extend finite PostgreSQL relation, type and routine catalogs | Open |
| [#818](https://github.com/cntryl/cassie/issues/818) | Resolve PostgreSQL object identifier pseudo-types | Open |
| [#819](https://github.com/cntryl/cassie/issues/819) | Add the selected snapshot isolation query profile | Open |
| [#820](https://github.com/cntryl/cassie/issues/820) | Support row locking and SKIP LOCKED claims | Open |
| [#821](https://github.com/cntryl/cassie/issues/821) | Extend scoped PostgreSQL transaction and session settings | Open |
| [#822](https://github.com/cntryl/cassie/issues/822) | Add session and transaction advisory locks | Open |
| [#823](https://github.com/cntryl/cassie/issues/823) | Execute UPDATE FROM and DELETE USING statements | Open |
| [#824](https://github.com/cntryl/cassie/issues/824) | Complete named constraint lifecycle and validation syntax | Open |
| [#825](https://github.com/cntryl/cassie/issues/825) | Complete PostgreSQL conflict target and conditional upsert syntax | Open |
| [#826](https://github.com/cntryl/cassie/issues/826) | Add PostgreSQL table creation and cloning forms | Open |
| [#827](https://github.com/cntryl/cassie/issues/827) | Add named enum types and typed enum values | Open |
| [#828](https://github.com/cntryl/cassie/issues/828) | Add composite, record and domain type semantics | Open |
| [#829](https://github.com/cntryl/cassie/issues/829) | Execute JSON element and recordset table functions | Open |
| [#830](https://github.com/cntryl/cassie/issues/830) | Extend SQL routines with signatures and set-returning results | Open |
| [#831](https://github.com/cntryl/cassie/issues/831) | Complete object ownership and privilege DDL | Open |
| [#832](https://github.com/cntryl/cassie/issues/832) | Enforce routine security context and scoped settings | Open |
| [#833](https://github.com/cntryl/cassie/issues/833) | Stage selected DDL changes transactionally | Open |
| [#834](https://github.com/cntryl/cassie/issues/834) | Execute data-modifying CTEs with RETURNING relations | Open |
| [#835](https://github.com/cntryl/cassie/issues/835) | Execute the selected procedural SQL language subset | Open |
| [#836](https://github.com/cntryl/cassie/issues/836) | Add anonymous procedural blocks and parameterized dynamic SQL | Open |
| [#837](https://github.com/cntryl/cassie/issues/837) | Add transactional row-trigger and constraint-trigger execution | Open |
| [#838](https://github.com/cntryl/cassie/issues/838) | Add session-local temporary tables and namespaces | Open |
| [#839](https://github.com/cntryl/cassie/issues/839) | Support atomic ALTER TYPE and multi-action table changes | Open |
| [#840](https://github.com/cntryl/cassie/issues/840) | Add LIST-partitioned relation lifecycle and routing | Open |
| [#841](https://github.com/cntryl/cassie/issues/841) | Extend sequence options, identity and value functions | Open |
| [#842](https://github.com/cntryl/cassie/issues/842) | Complete PostgreSQL index DDL and operator-class contracts | Open |
| [#843](https://github.com/cntryl/cassie/issues/843) | Add an explicit built-in extension compatibility catalog | Open |
| [#844](https://github.com/cntryl/cassie/issues/844) | Support the selected pgvector SQL and type identity profile | Open |
| [#845](https://github.com/cntryl/cassie/issues/845) | Persist SQL object comments and identity metadata | Open |
| [#846](https://github.com/cntryl/cassie/issues/846) | Extend finite PostgreSQL COPY and migration I/O forms | Open |
| [#847](https://github.com/cntryl/cassie/issues/847) | Qualify generated PostgreSQL ORM queries against the dialect profile | Open |
| [#848](https://github.com/cntryl/cassie/issues/848) | Qualify the complete generic SQL dialect corpus | Open |
| [#849](https://github.com/cntryl/cassie/issues/849) | Establish dialect workload resource and restart profiles | Open |

## Existing prerequisites and profile proposal

The prerequisites named by #791's current issue description are:

| Issue | State | Dependency role |
| --- | --- | --- |
| [#758](https://github.com/cntryl/cassie/issues/758) | Closed | Typed aggregate and encoded-column foundation. |
| [#761](https://github.com/cntryl/cassie/issues/761) | Open | Relational semantics qualification; blocks dependent compatibility work. |
| [#762](https://github.com/cntryl/cassie/issues/762) | Open | Bounded scalar subquery capability. |
| [#763](https://github.com/cntryl/cassie/issues/763) | Closed | Transaction atomicity and statement visibility contract. |
| [#768](https://github.com/cntryl/cassie/issues/768) | Open | Column layouts, codecs and generation publication qualification. |
| [#769](https://github.com/cntryl/cassie/issues/769) | Open | Constraints and mutation identity across ingress paths. |
| [#771](https://github.com/cntryl/cassie/issues/771) | Closed | Storage lifecycle and recovery publication boundaries. |
| [#776](https://github.com/cntryl/cassie/issues/776) | Closed | Vector value, metric and cross-interface ordering qualification. |
| [#777](https://github.com/cntryl/cassie/issues/777) | Open | Session catalogs and named client workflows. |
| [#779](https://github.com/cntryl/cassie/issues/779) | Open | Cross-path query invariant acceptance. |
| [#780](https://github.com/cntryl/cassie/issues/780) | Open | Bounded PostgreSQL wire-state qualification. |
| [#781](https://github.com/cntryl/cassie/issues/781) | Open | Final query-engine release/profile ledger. |

[PR #877](https://github.com/cntryl/cassie/pull/877) is the open, non-draft documentation proposal for [#792](https://github.com/cntryl/cassie/issues/792), based on `55b52e26ac570554ba394ec06d45568853e2866d`. It adds the finite capability matrix, priority/order, and decision gates in `docs/sql-dialect-profile.md`. Since this ledger's 2026-10-10 12:11 UTC capture, the user has approved the current #793 recommendations as written; exact per-family tags, codecs, typmods and migration details remain unspecified. This later approval does not close #792 or complete [#793](https://github.com/cntryl/cassie/issues/793). At capture, PR #877 had no review decision; `backend-test-matrix`, `Analyze (actions)`, and `Analyze (javascript-typescript)` passed, CodeQL was neutral, Rust analysis/backend-quality and five backend suites were running, and the documentation suite was queued. Hosted checks were therefore still pending.

## Conformance and operational gates

- [#848](https://github.com/cntryl/cassie/issues/848) remains open. It requires executed generic seeded differential fixtures for every selected profile cell, with a pinned PostgreSQL oracle and recorded values, NULL behavior, descriptors, errors, parameters, lifecycle and cleanup. Its prerequisite list includes #792, #793, the dialect children, and open qualification dependencies #761, #762 and #779. The profile proposal or a source mapping is not corpus evidence.
- [#849](https://github.com/cntryl/cassie/issues/849) remains open and depends on #848, [#8](https://github.com/cntryl/cassie/issues/8), and #781. It requires representative generic workloads plus resource, cancellation, contention, restart and recovery measurements on one immutable revision. It separates semantic completion from production promotion.
- [#8](https://github.com/cntryl/cassie/issues/8) remains open for profile-specific operational thresholds and repeated native Linux benchmark, recovery, scale and endurance evidence. Smoke results are diagnostics, not capacity objectives or an SLA.
- [#29](https://github.com/cntryl/cassie/issues/29) remains open and follows #8 evidence for any selected local diagnostic promotion. Cassie remains single-node; routing, placement, admission orchestration, replication and failover stay external.
- Supplemental finding [#850](https://github.com/cntryl/cassie/issues/850) is closed by merged [PR #853](https://github.com/cntryl/cassie/pull/853), merge commit `a07ff562dea8e5608aee129f5902a2dee29cac1a`. This resolves that reported mixed-numeric COALESCE defect; broader relational and dialect conformance remain open.

## Current disposition

This tracker and PR #877 make the dependency-ordered plan reviewable without resolving the child issues. #791 remains open: the 53 open children, open prerequisites, #848 differential corpus, and #849 resource/restart qualification still require their own accepted evidence. No support or Production-ready claim follows from this status refresh.

## Status source commands

The snapshot above was read from GitHub with these commands:

```sh
gh issue list --state all --limit 200 --json number,title,state,url
gh issue view 8 --json state,title,updatedAt,url
gh issue view 29 --json state,title,updatedAt,url
gh issue view 791 --json state,title,body,updatedAt,url
gh issue view 792 --json state,title,updatedAt,url
gh issue view 793 --json state,title,updatedAt,url
gh issue view 850 --json state,title,closedAt,url
gh pr view 877 --json title,state,isDraft,headRefName,headRefOid,baseRefOid,updatedAt,reviewDecision,statusCheckRollup,url
gh issue view 848 --json state,title,body,updatedAt,url
gh issue view 849 --json state,title,body,updatedAt,url
gh pr view 853 --json state,title,mergedAt,mergeCommit,url
```

This file is an explicit timestamped snapshot. Refresh it from GitHub before using it for later closure, prioritization, or readiness claims.
