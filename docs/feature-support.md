# Feature Support

This is the sole canonical behavior and status matrix for Cassie. Each capability has exactly one status:

- **Stable**: implemented, tested, documented, and supported within the current pre-release baseline.
- **Experimental**: implemented for documented cases, but correctness, resource bounds, evidence, or compatibility still has an open closure item.
- **Planned**: accepted future work that is not implemented as a supported contract.

`Production-ready` is not a feature status. It is an evidence classification owned by [Production Readiness](production-readiness.md).

The current beta support envelope consists of capabilities marked Stable. Experimental capabilities ship for evaluation with explicit limits and are not compatibility commitments. See [Production Readiness](production-readiness.md) for the release evidence bar.

## Ownership Boundary

Midge owns persistence, durability, and recovery mechanics. Cassie owns logical query layouts and query-visible failures, including SQL semantics, indexes, planning, execution, caching, cancellation, memory and result limits, and protocol error mapping. Cassie does not implement a parallel WAL, recovery engine, or storage abstraction.

Cassie is permanently a single-node query engine. Distributed SQL, cluster membership or management, replication, consensus, sharding and rebalancing, cross-node transactions, multi-node planning, remote query forwarding, and automatic cross-node repair are product non-goals. External systems may route to independent nodes, but that does not expand Cassie's execution or coordination boundary.

The only accepted Cassie-owned on-disk baseline marker is `cassie-midge-layout-v2`. A directory without that marker is rejected with a recreate diagnostic. There is no migration or legacy reader; recreating a directory created by an earlier layout version is required.

## Relational SQL

| Capability | Behavior | Status |
| --- | --- | --- |
| Core reads | `SELECT`, projection, aliases, expressions, `FROM`, `WHERE`, and searched or simple `CASE` expressions. | Stable |
| Predicates and nulls | Comparison, boolean logic, `IS NULL`, `IN`, `BETWEEN`, three-valued logic | Stable |
| Null-safe comparisons | `IS DISTINCT FROM` / `IS NOT DISTINCT FROM` over selected current scalar/ARRAY equality in query and DML expressions; native typed numeric filter/projection/aggregate-predicate kernels and projected-scan pruning, scalar/residual fallback otherwise (including joins), persisted-definition rejection. See [finite contract](null-safe-predicate-contract.md). | Experimental |
| Existing operator spelling | Compact and spaced `=`, `!=`, `<>`, `<`, `>`, `<=`, `>=`, `+`, `-`, `*`, and `/` share expression precedence, literal boundaries and parameter identity. Generic parser and bound-query witnesses live in `tests/parser_types/dialect_syntax.rs`; [#794](https://github.com/cntryl/cassie/issues/794) tracks qualification. | Experimental |
| Ordering and pagination | `ORDER BY`, null placement, `LIMIT`, `OFFSET` | Stable |
| SQL-standard pagination spelling | Selected integer `FETCH FIRST/NEXT … ROW/ROWS ONLY` and `OFFSET … ROW/ROWS`, including omitted FETCH count, checked parameters, and local/global set-query bounds. `WITH TIES` rejects 0A000 and remains pending. | Experimental |
| Deduplication | `DISTINCT`, `DISTINCT ON` | Stable |
| Aggregation | `count`, `sum`, `avg`, `min`, `max`, grouping and `HAVING` | Stable |
| Text `STRING_AGG` | TEXT/CHAR/VARCHAR values and delimiters; skips NULL values, treats a NULL delimiter as empty, and returns SQL NULL for empty or all-NULL input. Aggregate-local `FILTER`, `DISTINCT` over both arguments, and `ORDER BY` use the serial row fallback with query-memory admission; result type is TEXT. BYTEA `STRING_AGG` and ARRAY/JSON collection aggregates remain pending under [#811](https://github.com/cntryl/cassie/issues/811), whose array and JSON prerequisites are still open. | Experimental |
| Grouping sets | Single-expression `GROUPING SETS (expression)`, equivalent to `GROUP BY expression`; multiple or empty sets, `ROLLUP`, `CUBE`, `GROUPING()`, and `GROUP BY DISTINCT` remain unsupported. | Experimental |
| Join syntax | Inner, left, right, full outer, cross, lateral, apply, semi, and anti forms with legality-preserving planning | Experimental |
| Subqueries | Implemented table, predicate (including EXISTS), lateral, and correlated forms. | Experimental |
| Scalar expression subqueries | Scalar, row-valued, `[NOT] IN`, `op ANY/SOME/ALL (subquery)` and `op ANY/ALL (array)` expressions, correlated or uncorrelated, in the select list, `WHERE`, `JOIN ... ON`, `GROUP BY`, `HAVING`, `ORDER BY`, `LIMIT`/`OFFSET`, aggregate arguments, `VALUES` rows and `INSERT`/`UPDATE`/`DELETE` expressions. Zero rows yield NULL, more than one row raises `21000`, a wrong width raises `42601`; a scalar subquery reads at most two rows, an uncorrelated one runs once, and an unselected `CASE`/`COALESCE` branch never reports its error. `ARRAY(SELECT ...)` evaluates over the existing array carrier but its PostgreSQL array descriptor and text form wait on array support ([#807](https://github.com/cntryl/cassie/issues/807)); remaining positions are tracked in [#762](https://github.com/cntryl/cassie/issues/762). | Experimental |
| Common table expressions | Non-recursive and recursive `WITH`, including `WITH` before `INSERT`, `UPDATE` and `DELETE` and data-modifying `INSERT`/`UPDATE`/`DELETE`/`MERGE ... RETURNING` bodies: each runs exactly once against the statement's starting snapshot, the outer statement reads only its `RETURNING` rows, and the statement is atomic ([#834](https://github.com/cntryl/cassie/issues/834)). | Experimental |
| Typed VALUES sources | Selected DATE/TIME/TIMESTAMP/UUID/BYTEA and character casts, NULL/common-column typing, inferred/supplied parameters, text/binary formats and source-aware text conversion. Unbounded bpchar and character padding retain comparison/DISTINCT identity through projection and CTEs. Complete type/coercion/collation qualification remains pending. | Experimental |
| Set operations | `UNION`, `UNION ALL`, `INTERSECT`, `EXCEPT`; selected integer/text/NULL `INTERSECT ALL`, `EXCEPT ALL`, explicit DISTINCT, SELECT ALL and VALUES branches now match native multiplicity and ordering. Wider coercion qualification remains pending under #800. | Experimental |
| Window functions | Ranking, offset, value functions, and documented row frames | Experimental |
| Views | Read-only views and nested views | Stable |
| User functions | Scalar UDFs with declared volatility | Experimental |
| Procedures | `CREATE PROCEDURE`, `CALL`, and `DROP PROCEDURE` for the narrow one-statement contract in [Limited Procedures and CALL](procedure-support.md); not a stored-procedure business-logic platform | Stable |
| Types and casts | Original scalar, JSON, vector and bounded array carriers plus native `real`, exact `numeric`, extended temporal (`timetz`, `timestamptz`, `interval` and precision-qualified forms), network (`inet`, `cidr`, `macaddr`, `macaddr8`), `pg_lsn` and `bit`/`bit varying` families. Each family has PostgreSQL type identity, selected text/binary codecs and its implemented operators/functions; support does not imply every cast, overload or matching array form. Selected INT2/INT4/INT8 ↔ BYTEA casts preserve declared widths, zero extension, signed boundaries, NULLs and native overlength errors. See [Type Contract](type-contract.md) and [Extended Types](extended-types.md). Complete assignment, session output, cross-family coercion and broader array qualification remain pending. | Experimental |
| Conditional scalar expressions | Selected `NULLIF`, `GREATEST` and `LEAST` over existing integer, FLOAT, TEXT and BOOLEAN families; see [Conditional expressions](conditional-expressions.md) for coercion, NULL, descriptor and exclusion rules. | Experimental |
| Reserved identity column | `_id` names the internal document identity and cannot be declared or dropped; a table may declare its own `id` column and query it as an ordinary column | Stable |

The [finite SQL type and wire contract](type-contract.md) records current logical
types, private wire identities, codec and metadata boundaries, exactness, NULL
meaning and explicit exclusions. Its qualification does not promote the broader
types/casts family.

Table declarations accept `CHARACTER VARYING(n)`, `CHAR VARYING(n)`,
`NATIONAL CHARACTER VARYING(n)` and `NCHAR VARYING(n)` as the same OID 1043
identity, preserving typmod `n + 4`. The pgwire regression
[`should_bind_varying_character_columns_with_native_metadata`](../tests/pgwire_extended/grammar/parameters.rs)
checks the descriptors, values and trailing `NOT NULL` constraints for all four
forms. Other type-name and cast gaps remain under [#796](https://github.com/cntryl/cassie/issues/796).

Table declarations accept `TIME` and `TIMESTAMP` with optional precision and
explicit `WITH TIME ZONE` or `WITHOUT TIME ZONE` suffixes. The pgwire test
[`should_bind_temporal_zone_columns_with_native_metadata`](../tests/pgwire_extended/types/temporal/metadata.rs)
creates the four forms with trailing `NOT NULL`, inserts and reads values, and
checks OIDs, typmods and text results. Other multi-word type-name and cast gaps
remain under [#796](https://github.com/cntryl/cassie/issues/796).

The #796 extended-protocol witness
[`should_bind_the_selected_four_type_cast_fixture_through_extended_protocol`](../tests/pgwire_extended/grammar/parameters.rs)
executes inferred `uuid`, `jsonb`, `numeric(18,2)`, and `uuid[]` casts through
Parse, Describe, Bind, and Execute, including rounded numeric input and
all-NULL parameters. It records JSONB under the JSON OID and UUID arrays under
Cassie's private array OID and JSON-array text representation. Matching array
source and target types preserve the existing array value carrier; this does
not qualify PostgreSQL array ABI or other array casts.

Operator spelling preserves signed literals, exponent signs, quoted identifiers
and strings, and nested comment separators. Existing `count(*)` arguments and
raw nested query text remain unchanged. Regex, JSON, array and concatenation
operators remain outside this selected lexical contract; expanded cast syntax
is tracked separately in [#796](https://github.com/cntryl/cassie/issues/796).
Malformed exponent tokens fail parsing even when a similarly named quoted
column exists; quoted column references and exponent-like identifiers remain
ordinary identifier references.
The lexical scanner skips dollar-quoted regions without admitting dollar-quoted
scalar string literals: those still fail parameter parsing. A single sign on a
numeric literal is supported; repeated unary-sign chains such as `n+++2` and
`n-+-2` remain excluded. Lexical parity does not establish PostgreSQL's broader
literal, unary-expression or postfix-cast grammar.

### JSON Document Null and SQL NULL

For a declared `JSON` field, an absent key in the stored document represents SQL
`NULL`; an explicit key whose value is JSON `null` represents a non-NULL JSON
document value. SQL `NULL` writes through `INSERT`, `UPDATE`, or CSV `COPY` omit
the field from the raw document payload. JSON text `'null'`, a non-empty CSV
token `null`, typed JSON binds, and REST document input with `"field": null`
preserve the explicit JSON value. REST reads therefore omit SQL-NULL fields and
return JSON-null fields with their key present.

SQL treats explicit JSON `null` as a value: it satisfies `NOT NULL`,
`field IS NULL` is false, and `COUNT(field)` includes it. Query projection of an
absent field returns SQL `NULL`. Rows written by earlier builds that encoded
JSON `null` with the generic SQL-NULL tag are interpreted as SQL `NULL` because
that historical representation cannot distinguish the values.

JSON responses cannot encode floating-point `NaN`, positive infinity, or
negative infinity. REST query and vector-search responses return an error when
a result contains one of these values; SQL `NULL` continues to serialize as
JSON `null`. Materialized projection refresh validates its complete output
before replacing stored rows. A rejected value leaves the previous version's
rows in place, and projection freshness determines whether those rows can
serve reads.

### ARRAY Ordering

Declared one-dimensional ARRAY columns compare elementwise in ordered predicates,
`ORDER BY` (including top-k and window ordering), and `MIN`/`MAX`. The first
unequal element determines order; an equal prefix sorts before a longer array.
NULL elements sort after non-NULL elements, independently of top-level SQL NULL
ordering. Integer elements retain exact comparisons. JSON columns retain their
existing JSON comparison behavior, even when their values are arrays. ARRAY
stored values keep the existing JSON representation; scalar-function conversions
are unchanged. This adds no new literal or cast syntax, nested ARRAY types, or
storage encoding.

### Reserved Identity Column

Every stored document carries an internal identity. It is exposed under the reserved name `_id`, which cannot be declared by `CREATE TABLE` or `ALTER TABLE` and cannot be dropped.

A table may declare its own `id` column. When it does, `id` is an ordinary column everywhere: `SELECT`, `WHERE`, `ORDER BY`, `GROUP BY`, `UPDATE`, `DELETE`, and `ALTER TABLE DROP COLUMN` all resolve the user's stored value. When a table does not declare `id`, a bare `id` reference resolves to the internal identity, which is the long-standing default and the value `SELECT *` returns in its leading column.

This resolution is uniform across single-table reads, joins, set operations, derived tables, CTE references, and `INSERT ... SELECT`. A subquery or CTE that projects an explicit `id` output column passes that column to its consumer unchanged.

`_id` can be named directly against a base table in a select list, `WHERE`, and `ORDER BY`, and always returns the internal identity, including on a table that declares its own `id`. `ORDER BY <indexed column>, _id` keeps the ordered scalar-index read path with the identity as a deterministic tiebreaker. A derived relation exposes only the columns it projects, so `id` or `_id` that its body does not project is an unresolvable column reference rather than NULL.

Adding an `id` column with `ALTER TABLE ... ADD COLUMN id ...` switches a bare `id` on that table from the internal identity to the new column, which is NULL for rows written before the change; `_id` is unaffected.

REST document endpoints report the internal identity as `"id"` in their JSON body. That field is the SQL `_id` of the row, not a declared `id` column.

## Mutation and Catalog

| Capability | Behavior | Status |
| --- | --- | --- |
| DML | `INSERT`, PostgreSQL-style `ON CONFLICT`, `UPDATE`, `DELETE`, `RETURNING`, CSV copy ingestion | Experimental |
| Transactions | Begin, commit, rollback, savepoints, read-your-writes and the bounded implicit Query/Sync profile with atomic healthy handoff; standalone DDL and explicit table COPY boundaries follow the [wire transaction contract](pgwire-transaction-contract.md). | Experimental |
| Database and schema scope | Databases, schemas, persisted `search_path`, qualified names, and administrator-managed `CONNECT` grants with live-session revalidation | Stable |
| Tables and constraints | Table DDL, name-idempotent `CREATE TABLE IF NOT EXISTS`, and `DROP TABLE IF EXISTS` as a no-op for a missing relation; defaults, unique, check, foreign key | Experimental |
| Scalar indexes | Primary, secondary, composite, unique, covering, partial, expression, and name-idempotent `CREATE INDEX IF NOT EXISTS` | Experimental |
| Physical ColumnStore tables | Field-key table layout in the owning Midge database family; separate from Stable CBM2 sidecars and the Planned common typed-column pipeline. Layout, lifecycle and typed-reader qualification remain under [#768](https://github.com/cntryl/cassie/issues/768). | Experimental |
| `information_schema.tables` | Stable named-client table/view discovery columns documented in the catalog support contract | Stable |
| `information_schema.columns` | Stable named-client column discovery columns documented in the catalog support contract | Stable |
| Remaining virtual catalogs | PostgreSQL-like and Cassie runtime views outside the stable named-client subset; no PostgreSQL-internal parity claim | Experimental |
| Projection lifecycle | Materialized projection create, refresh, checkpointed replay, version build, verification, activation, rollback by reactivation, repair, and drop on one local instance, as defined in [Materialized Projection Lifecycle](materialized-projection-lifecycle.md). Interrupted or failed builds never become query-visible; stale or unproven projections fall back to authoritative source rows. Column-store table mode, remote replay coordination, and automatic activation are outside this surface. | Stable |
| Verification and repair | Hashes, manifests, local repair planning and audit for materialized projections; see [Materialized Projection Lifecycle](materialized-projection-lifecycle.md). No general (non-projection) index verification or repair exists. | Experimental |

## Retrieval and Analytics

| Capability | Behavior | Status |
| --- | --- | --- |
| Full-text search | Persisted posting-block reads, exact BM25 scoring, snippets, bounded candidate fetches, transaction overlays, and labelled artifact fallback | Stable |
| Exact vector search | Cosine, dot, and L2 scoring with streaming bounded top-k, cancellation, and hard memory limits | Stable |
| HNSW | Persisted graph point-read candidates, deterministic bounded expansion, and exact source-row reranking | Stable |
| IVFFlat | Persisted centroid membership-prefix candidates, deterministic probes, and exact source-row reranking | Stable |
| Hybrid retrieval | Persisted text, vector, and structured candidate intersection before exact final scoring | Stable |
| OpenAI embeddings | Controlled, response-bounded OpenAI protocol with mocked authentication and rate-limit coverage; third-party availability is not guaranteed | Experimental |
| OpenAI-compatible embeddings | Controlled, response-bounded configurable OpenAI-compatible protocol with mocked authentication and rate-limit coverage; endpoint compatibility and availability are operator-owned | Experimental |
| TEI embeddings | Controlled, response-bounded TEI protocol with deterministic local-server coverage; endpoint compatibility and availability are operator-owned | Experimental |
| Ollama embeddings | Controlled, response-bounded Ollama protocol with deterministic local-server coverage; endpoint compatibility and availability are operator-owned | Experimental |
| Voyage embeddings | Controlled, response-bounded Voyage protocol with mocked authentication and rate-limit coverage; third-party availability is not guaranteed | Experimental |
| Cohere embeddings | Controlled, response-bounded Cohere protocol with mocked authentication and rate-limit coverage; third-party availability is not guaranteed | Experimental |
| Local deterministic embeddings | In-process deterministic hashing protocol with configured model label and dimensions; no external service or learned semantic model | Stable |
| Time-series indexes | Ordered partition/range lookup with fixed-duration positive integer `minute(s)`, `hour(s)`, or `day(s)` bucket widths. The promoted evidence matrix uses 15 minutes, 1 hour, and 1 day. Buckets are fixed UTC durations; calendar month, week, local-time, and daylight-saving semantics are unsupported. Missing, stale, corrupt, or interrupted derived state falls back atomically to authoritative rows. | Stable |
| Time-series rollups and retention | Explicit rollup refresh and retention-policy enforcement over time-series data | Experimental |
| Catalog scalar helpers | Exact `pg_size_bytes(text)`, bigint `pg_size_pretty`, and selected encoding-name lookup; the `pg_size_pretty(numeric)` overload, native NAME parameter ABI and physical size calculation are outside this checkpoint. See [catalog support](catalog-support.md). | Experimental |
| Column-batch analytics | Stable CBM2 batches provide deterministic automatic plain, constant, typed RLE, dictionary, 128-value frame-of-reference, FSST UTF-8 symbol streams, and ALP decimal-scaled float blocks, summary pruning, late materialization, encoded scans, exact filtered aggregate acceleration, and generation-fenced range copy-on-write maintenance. FSST uses manifest codec tag 6 and codec version 1 with at most 256 deterministic symbols and a bounded 64 KiB symbol table; selective projection validates the complete table, framing, indices, decoded sizes, and UTF-8 while materializing only selected values. ALP uses manifest codec tag 5 and codec version 1, encodes only finite bit-exact values, and leaves signed zero or non-exact values on plain encoding. Unsupported, stale, corrupt, unknown-version, or over-limit derived state falls back atomically to authoritative rows. Codec selection is not a SQL or configuration surface. | Stable |
| Graph traversal | Neighbor expansion and shortest paths | Experimental |

## Planning, Execution, and Interfaces

| Capability | Behavior | Status |
| --- | --- | --- |
| Option expansion | Selected one-dimensional logical TEXT[] inputs to pg_options_to_table FROM relations, with native TEXT outputs and selected errors. Native array/RECORD ABI, bounds, dimensions and SQL array assignment remain pending. | Experimental |
| Runtime information | Actual recovery/JIT/NUMA capabilities and Rust Unicode version; NULL compression for materialized SQL values; empty loaded-module FROM relation with three TEXT columns. Scalar RECORD projection and complete system/catalog ABI remain pending. | Experimental |
| Catalog read models | Selected database-scoped table/view/sequence discovery, ordinary column identity/generation flags, and column-name/empty-row shapes for 26 absent object registries. Ownership, definitions and wider DDL remain pending. | Experimental |
| Object identifiers | Native `oid` plus `regclass`, `regtype`, `regproc`, `regprocedure`, `regoper`, `regoperator`, `regnamespace`, `regrole`, `regcollation`, `regconfig` and `regdictionary`: PostgreSQL text input/output, search-path resolution and visibility-qualified output, `to_reg*` lookups, `pg_typeof`, integer casts with `oid` range errors and 4-byte binary wire codecs. Tables, views, sequences, schemas and roles keep a stored OID across rename and restart; `pg_class`/`pg_namespace` report the same OIDs. Built-in types, functions, operators, collations and text-search objects use PostgreSQL 18.6 OIDs. User-defined types, routines and operators are not resolvable. | Experimental |
| Built-in extension compatibility | Planned finite `vector` and `pgcrypto` compatibility shims: extension catalog/lifecycle behavior and `pgcrypto.digest(text|bytea, 'sha256')`; vector SQL, type and retrieval semantics are owned by #844. Cassie does not load native extension libraries or claim arbitrary extension ABI support. See #843. | Planned |
| Plan cache | Session-safe reusable parsed and physical plans | Experimental |
| Execution-result cache | Context-isolated, epoch-invalidated, byte-bounded query results | Experimental |
| Cost planning | Relational join enumeration, deterministic statistics fallbacks, physical properties, and access-path selection | Experimental |
| Adaptive scalar reads and join switching | Opt-in feedback-informed scalar read selection and vectorized-to-merge inner/left equi-join switching under the named profiles | Stable |
| Broader adaptive planning | Candidate expansion, other operator pairs, automatic profile selection, and default enablement | Experimental |
| Pull execution | Bounded batch streams and early termination | Experimental |
| Common typed-column relational execution | The first private scan/filter/projection slice carries typed columns through selected collection reads, with native numeric/Boolean/NULL kernels, bounded scalar expressions and a final admitted row/wire handoff. [Finite acceptance and fallback boundaries](typed-pipeline-acceptance.md) preserve specialized access paths, filtered CBC2, staged visibility, parallel scans and tight memory profiles. The complete join/aggregate/order/set/window pipeline remains absent; later stages and promotion gates are defined in [Query Engine Target 1](query-engine-target.md). | Planned |
| Query controls | Deadline, cancellation, SQL complexity, transport write, result, candidate, worker, and memory bounds | Experimental |
| Configurable parallelism | Shared worker permits with deterministic merges | Experimental |
| Pgwire | Primary SQL interface; detailed contract in compatibility documentation | Experimental |
| Limited procedures and `CALL` | One persisted Cassie SQL statement with positional arguments and pgwire command metadata; PL/pgSQL, triggers, dynamic SQL, transaction control, recursion, and business-procedure workflows are unsupported. See [Limited Procedures and CALL](procedure-support.md). | Stable |
| REST | Secondary administrative and resource API | Experimental |
| Admin UI | Supported local operational interface over REST with login, database navigation and creation, SQL editing and completion, validation, explain, execution, result inspection, cancellation, and logout workflows. Native Askr shell, control, query/mutation, error-boundary, resizable-panel, and Monaco composition is covered by 116 repository tests, 20 desktop/mobile mock-browser cases, 2 real-Cassie production-browser cases, axe state coverage, and four committed visual baselines on Askr `0.2.1`, Askr UI `0.2.0`, `@askrjs/themes` `0.2.1`, and `@askrjs/monaco` `0.2.0`. Dynamic schema fan-out retains a narrow application controller pending [askrjs/askr#327](https://github.com/askrjs/askr/issues/327); it does not duplicate the Askr query cache. | Stable |

The [Query Engine Target 1](query-engine-target.md) and [invariant ownership ledger](query-engine-invariant-ownership.md) define finite delivery and qualification gates without promoting these support labels.

The [Query Promotion Evidence](query-promotion-evidence.md) inventory records deterministic
baselines and remaining promotion owners for the Experimental query families above. Completing an
evidence row does not independently change that family's support status.

## Intentional Limits

Cassie does not claim full PostgreSQL syntax or catalog parity, distributed execution or cluster management, trigger-based application logic, or general OLTP behavior. Unsupported syntax and resource exhaustion return deterministic query-visible errors instead of silently changing semantics.
