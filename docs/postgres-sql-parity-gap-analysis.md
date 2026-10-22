# PostgreSQL SQL Feature Parity Gap Analysis (historical)

> **Superseded survey.** This document records the former proposal to pursue
> broad PostgreSQL feature parity. That target was closed as superseded. It is
> retained as historical input only; it does not define current Cassie scope.
> Use [Product Roadmap](product-roadmap.md) and
> [Feature Support](feature-support.md) for current product and behavior.

Baseline: `main` at `55b52e26`, PostgreSQL 18 reference dialect, 2026-10-10.
Milestone: [Postgres SQL feature parity](https://github.com/cntryl/cassie/milestone/6). Tracker: #791.

This document maps every PostgreSQL 18 SQL feature area to one of three outcomes:
**supported** today, **owned** by an open milestone issue, or **excluded** permanently. It is a scope record, not support evidence; [Feature Support](feature-support.md)
remains the canonical status of shipped behavior, and #848 owns differential conformance evidence.

## Selected scope and priority

The [PostgreSQL 18 delivery contract](postgresql18-delivery.md) supersedes the
prior closed-scope rule and permanent exclusions below. Complete PostgreSQL
At the time of this 2026-10-10 survey, PostgreSQL 18.6 core SQL and full PL/pgSQL
were listed as the top priority. That goal was later closed as superseded. The
survey baseline remains
historical; the owner mappings below are updated for the expanded scope.
Current Feature Support remains the authority for implemented behavior; these
survey labels are narrower historical observations, not whole-family parity.

## Coverage matrix

| PostgreSQL area | Cassie today | Owner |
|---|---|---|
| **Statements** | | |
| SELECT, INSERT, UPDATE, DELETE, RETURNING, ON CONFLICT | Supported | #825 (conflict targets), #869 (EXISTS in RETURNING) |
| MERGE (incl. `BY SOURCE`, `RETURNING`, `merge_action()`) | Absent | #823 |
| TRUNCATE | Absent | #823 |
| UPDATE FROM, DELETE USING, `INSERT DEFAULT VALUES`, `RETURNING OLD/NEW` | Absent | #823 |
| Data-modifying CTEs | Supported (top-level `WITH`; once per statement, one snapshot, atomic) | #834 |
| COPY FROM STDIN csv | Supported | — |
| COPY text format, COPY TO, binary, query source, options | Absent | #846 |
| EXPLAIN [ANALYZE] | Supported | — |
| EXPLAIN option list (`FORMAT JSON`, `VERBOSE`, …) | Absent | #821 |
| PREPARE / EXECUTE / DEALLOCATE (SQL level) | Absent (protocol-level only) | #821 |
| DECLARE / FETCH / MOVE / CLOSE cursors | Absent | #821 |
| DISCARD, RESET, SET LOCAL, SHOW ALL | Absent | #821 |
| LISTEN / NOTIFY / UNLISTEN, `pg_notify` | Rejected | #822 |
| LOCK TABLE | Absent | #820 |
| VACUUM, ANALYZE, CHECKPOINT, REINDEX, CLUSTER | Complete real maintenance unqualified | #842 (Midge-backed effects required) |
| DO blocks, dynamic EXECUTE | Absent | #836 |
| COMMENT ON | Absent | #845 |
| **Query clauses** | | |
| INNER/LEFT/RIGHT/FULL/CROSS/LATERAL joins, aliases | Supported | — |
| NATURAL JOIN, JOIN USING | Absent | #800 |
| Qualified relation wildcard `t.*` | Experimental for base tables, CTEs and derived relations; unknown qualifiers fail binding and existing wildcard identity rules apply | #761 |
| UNION [ALL], INTERSECT, EXCEPT | Supported | — |
| INTERSECT ALL, EXCEPT ALL | Selected integer/text/NULL multiplicity qualified, including VALUES branches and explicit DISTINCT; wider coercions pending | #800 |
| VALUES relations, row constructors, tuple comparison | Absent | #800 |
| WITH [RECURSIVE] | Supported | — |
| CTE `[NOT] MATERIALIZED`, `SEARCH`, `CYCLE` | Absent | #801 |
| EXISTS (incl. correlated) | Supported | — |
| Scalar, IN, ANY/SOME/ALL subqueries | Supported (scalar, row, membership and quantified forms; correlated or uncorrelated; `ARRAY(SELECT ...)` output waits on #807) | #762 |
| GROUP BY, HAVING, DISTINCT, DISTINCT ON | Supported | — |
| GROUP BY output aliases and ordinals outside grouping sets | Supported for explicit select-list expressions, with input-column name precedence and PostgreSQL's invalid-position state. [Executed tests](../tests/sql_queries/aggregates/grouping/group_by_alias_ordinal/semantics.rs) cover aliases, ordinals, expression aliases, precedence and invalid positions. Grouped `SELECT *` remains pending because wildcard projection is not supported with grouping. | #804 |
| GROUP BY primary-key functional dependency | Partial: single-column primary keys on direct base tables; aliased and joined forms pass focused local tests. Composite primary-key DDL remains unsupported. | #804 |
| GROUPING SETS, ROLLUP, CUBE, `GROUPING()`, `GROUP BY DISTINCT` | Partial: single-expression `GROUPING SETS (expression)` is supported and matches `GROUP BY expression`; the focused test covers duplicate and NULL keys. Multiple/empty sets, ROLLUP, CUBE, `GROUPING()`, and `GROUP BY DISTINCT` remain absent. PostgreSQL 18.6 returns `-2, 0, 5, 7, NULL` for the SQL-002 corpus values; [Cassie test](../tests/sql_queries/aggregates/grouping/single_grouping_set/semantics.rs). | #804 |
| ORDER BY NULLS FIRST/LAST, LIMIT/OFFSET with parameters | Supported | — |
| `FETCH FIRST … WITH TIES`, `OFFSET … ROWS` | Absent | #800 |
| FOR UPDATE/SHARE, NOWAIT, SKIP LOCKED | Absent | #820 |
| Window functions (row_number, rank, dense_rank, lag, lead, first/last_value) | Partial | #806 (rest of family, named WINDOW, aggregates OVER, frame qualification) |
| **Expressions and operators** | | |
| CASE, CAST, `::`, IS [NOT] NULL, IN list, BETWEEN, LIKE | Supported | — |
| IS [NOT] DISTINCT FROM | Experimental | #798 |
| Chained casts, type modifiers, multi-word type names, typed literals | Partial | #796 |
| `\|\|`, `%`, `^`, bitwise, BETWEEN SYMMETRIC, IS TRUE/UNKNOWN, `E''`, `U&''`, dollar quotes, COLLATE | Absent | #796 |
| ILIKE, NOT LIKE, SIMILAR TO, POSIX regex, LIKE ESCAPE | Absent | #799 |
| Array literals, subscripts, slices, containment, array ANY/ALL | Absent | #807 |
| JSON operators and jsonpath | Absent | #809 |
| **Data types** | | |
| smallint, int, bigint, float8, boolean, text, char, varchar, uuid, bytea, date, time, timestamp, json, 1-D arrays, vector | Supported | #844 (pgvector identity), #793 (standard array OIDs) |
| Exact NUMERIC / DECIMAL | Inexact (stored as float) | #812 |
| real / float4 | Absent | #793 |
| TIMESTAMPTZ, INTERVAL, AT TIME ZONE | Absent | #814 |
| JSONB as a distinct type | Alias of json | #793, #809, #810 |
| Enum types | Absent | #827 |
| Range and multirange types | Absent | #905 |
| Collations, including ICU | Absent | #796 |
| Composite, record, domain types | Absent | #828 |
| bit / varbit | Absent | #816 |
| inet, cidr, macaddr, macaddr8 | Absent | #816, #793 |
| tsvector, tsquery | Absent | #799, #793 |
| OID and reg* types | Absent | #818 |
| **Functions** | | |
| count, sum, avg, min, max | Supported | — |
| Aggregate DISTINCT, FILTER, ORDER BY | Absent | #804 |
| bool_and, bool_or | Absent | #805 |
| Statistical, ordered-set, hypothetical-set, bitwise aggregates | Absent | #806 |
| string_agg, array_agg, json(b)_agg, json(b)_object_agg | Absent | #811 |
| coalesce, nullif, greatest, least | Supported | — |
| String functions | Partial (length, lower, upper, substring, trim, concat) | #803 |
| Regex functions | Absent | #799 |
| Math functions | Partial (abs) | #813 |
| Date/time functions and clocks | DEFAULT-only | #815 |
| Array functions | Absent | #807 |
| JSON functions, SQL/JSON constructors and query functions | Absent | #810 |
| JSON table functions, JSON_TABLE | Absent | #829 |
| UNNEST, generate_series, generate_subscripts, ROWS FROM | Absent | #808 |
| bytea, hashing, encoding, UUID functions | Absent | #816 |
| Text search functions (`to_tsvector`, `@@`, `ts_rank`, …) | Absent (Cassie `search()` instead) | #799 |
| Sequence functions (nextval, currval, setval, lastval) | DEFAULT-only nextval | #841 |
| System information and administration functions | Partial | #817, #818 |
| **DDL** | | |
| CREATE/DROP TABLE, VIEW, SEQUENCE, DATABASE, SCHEMA, INDEX | Supported | — |
| ALTER TABLE single action | Supported | #839 (multi-action, ALTER TYPE, rename/move) |
| CREATE OR REPLACE VIEW, ALTER VIEW, updatable views | Absent | #839 |
| Multi-name DROP, CASCADE/RESTRICT, IF EXISTS everywhere | Partial | #839 |
| CREATE TABLE AS, LIKE, SELECT INTO, UNLOGGED | Absent | #826 |
| Materialized views | Absent (Cassie projections instead) | #826 |
| Generated and identity columns, OVERRIDING | Absent | #826, #841 |
| PK, UNIQUE, FK actions, NOT NULL, DEFAULT | Supported | — |
| General CHECK expressions | Partial (column vs constant) | #824 |
| Named constraints, NOT VALID, VALIDATE, DEFERRABLE, NULLS NOT DISTINCT | Partial | #824 |
| Index DDL: CONCURRENTLY, operator classes, ordered keys | Partial | #842 |
| Sequence options, ALTER SEQUENCE, OWNED BY | Absent | #841 |
| Temporary tables | Absent | #838 |
| LIST, RANGE, HASH partitioning | Absent | #840 |
| CREATE TYPE (enum, composite, range), DOMAIN | Absent | #827, #828, #905 |
| CREATE EXTENSION | Absent | #843 |
| Transactional DDL | Partial: selected explicit schema/table/scalar-index lifecycle; implicit and unselected DDL remain standalone only | #833 |
| **Routines and triggers** | | |
| Scalar SQL functions, single-statement procedures, CALL | Supported | — |
| Overloads, defaults, named args, RETURNS TABLE/SETOF, OR REPLACE | Absent | #830 |
| Full PL/pgSQL | Absent | #835 (subset is intermediate) |
| SECURITY DEFINER, routine SET | Rejected | #832 |
| Row, statement, constraint, INSTEAD OF and event triggers | Absent | #837 |
| **Security** | | |
| Roles with LOGIN/PASSWORD, GRANT CONNECT | Supported | — |
| Object privileges, ALTER OWNER, default privileges, membership | Rejected | #831 |
| SET ROLE, SET SESSION AUTHORIZATION, REASSIGN/DROP OWNED | Rejected | #831 |
| Row-level security, CREATE POLICY | Rejected | #831 |
| **Transactions and session** | | |
| BEGIN/COMMIT/ROLLBACK, SAVEPOINT, READ COMMITTED | Supported | — |
| REPEATABLE READ, SERIALIZABLE, READ ONLY | Rejected | #819 |
| SET/RESET/SET LOCAL for search_path, TimeZone, timeouts | Partial | #821 |
| Advisory locks | Absent | #822 |
| **Catalogs** | | |
| information_schema tables/columns/views/schemata/sequences/constraints | Supported | — |
| pg_catalog database/namespace/class/attribute/index/attrdef/constraint/roles/settings/type | Supported | #817 (column completeness) |
| pg_proc, pg_depend, pg_sequence, pg_enum | Absent | #817 |
| Client-introspection relations (pg_description, pg_inherits, pg_am, pg_trigger, pg_policy, pg_tables, pg_views, pg_stat_activity, information_schema routines/parameters/…) | Absent | #817 |
| pg_extension, pg_available_extensions | Absent | #843 |
| **Cross-cutting** | | |
| Implicit coercion and unknown-literal resolution | Implicit | #793 |
| Generated ORM queries (Prisma) | Unqualified | #847 |
| Differential corpus against a pinned PostgreSQL oracle | Absent | #848 |
| Workload resource and restart profiles | Absent | #849 |

Engine-qualification prerequisites that stay in their Query engine milestones: #761, #779, #780, #777.

## Additional in-scope owners

| PostgreSQL area | Complete acceptance | Owner |
| --- | --- | --- |
| Shared type/OID/coercion/catalog runtime | Incomplete | #904 |
| XML and SQL/XML | Absent/unqualified | #906 |
| Geometric types/functions | Absent/unqualified | #907 |
| money | Absent/unqualified | #908 |
| pg_lsn SQL values | Absent/unqualified | #816 |
| TABLESAMPLE | Absent/unqualified | #800 |
| Inheritance | Absent/unqualified | #909 |
| Exclusion and temporal constraints | Absent/unqualified | #824; #905 range primitives |
| SQL-defined aggregates | Absent/unqualified | #910 |
| SQL-defined casts/conversions | Absent/unqualified | #911 |
| Operators/operator classes/families | Absent/unqualified | #912; #842 index integration |
| Rewrite rules | Absent/unqualified | #913 |
| Extended statistics | Absent/unqualified | #914; #842 ANALYZE primitives |
| Local two-phase transactions | Absent/unqualified | #915 |
| Large objects | Absent/unqualified | #916 |

## Architectural exceptions

Only native C/plugin ABIs, other procedural-language runtimes, foreign storage/
federation, replication and PostgreSQL-specific physical server management such
as tablespaces and ALTER SYSTEM are outside the target. Excluded forms reject
deterministically under the recorded exception; #817 owns truthful introspection.
No missing in-scope behavior becomes an exception merely because it is absent.
Local two-phase transactions, large objects, full PL/pgSQL and real Midge-backed
maintenance are explicitly included. Full numeric/array/timezone semantics are
required; finite earlier selections remain intermediate slices.

## Resolved scope decisions

The expanded user plan supersedes all earlier closed-scope/exclusion choices.
Ranges/multiranges move from #828 to #905; composite/record/domain remain #828.
The [delivery record](postgresql18-delivery.md) separates built-in range
primitives from later custom-range/index obligations to avoid the former
constraint/table/type dependency cycle.

## Historical survey review record

Inventory: source survey of `src/sql/ast.rs` (51 statement variants), `src/sql/functions.rs`
(scalar registry), `src/types/schema.rs` (17 data types) and `src/catalog/virtual_views.rs`,
cross-checked against `docs/feature-support.md` and `docs/sql-dialect-profile.md`.

Jev (`jev-latest`), request/response under `target/pg-parity/`:

| Question | Result | Action |
|---|---|---|
| Is a mainstream-client PostgreSQL area missing from the plan? (noul) | 0.62 | At or above 0.30: ran a focused candidate probe |
| Which candidate area is missing? (choice) | client-introspection catalogs, 0.87 | Folded into #817 |
| After that fold, which candidate remains? (choice) | COPY text format 0.38; type resolution 0.23; object rename DDL 0.12 | Folded into #846, #793, #839 |
| Which exclusion most risks blocking ORMs? (choice) | network types, 0.27 | User brought it into scope (#816) |
| Which consolidated gap duplicates an existing issue? (choice) | SQL/JSON 0.23, operators 0.20 (low confidence) | Both folded into existing owners (#810/#829, #796/#799) |

### Item-level sweep (2026-10-10)

After the area-level pass, three independent sweeps compared each entry in the PostgreSQL 18 reference against current support and the owner issues' scope text:

- every page in SQL Commands, synopsis by synopsis;
- Functions and Operators §9.1–9.16;
- §9.17–9.31, with data types (chapter 8), SQL syntax (chapter 4), queries (chapter 7), DDL (chapter 5) and the PostgreSQL 18 release notes.

An item counted as covered only when an owner issue named it or unambiguously encompassed it. The sweeps found 219 uncovered items. After cross-sweep duplicates were removed, 211 were appended to the `Added scope` checklists of 45 owner issues, together with 17 items from the follow-up review: transaction aliases and chaining, `SET CONSTRAINTS`, `WITH` before DML, pg_dump DDL forms, `NOT ENFORCED`, `any_value`, enum and normalization functions, client encodings, range types and collations. #792 picked up an explicit rejection policy for C-language base types. Sweep output is kept under `target/pg-parity/sweep-*.json`.

A residual-gap probability stays high (0.74) because the PostgreSQL surface is large. The
differential corpus #848 and ORM qualification #847 are the mechanisms that catch residual forms;
they attach findings to owner issues under the scope rule.
