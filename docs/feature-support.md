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
| Null-safe comparisons | `IS DISTINCT FROM` / `IS NOT DISTINCT FROM` over selected current scalar/ARRAY equality in query and DML expressions; scalar/residual fallback, persisted-definition rejection. See [finite contract](null-safe-predicate-contract.md). | Experimental |
| Existing operator spelling | Compact and spaced `=`, `!=`, `<>`, `<`, `>`, `<=`, `>=`, `+`, `-`, `*`, and `/` share expression precedence, literal boundaries and parameter identity. Generic parser and bound-query witnesses live in `tests/parser_types/dialect_syntax.rs`; [#794](https://github.com/cntryl/cassie/issues/794) tracks qualification. | Experimental |
| Ordering and pagination | `ORDER BY`, null placement, `LIMIT`, `OFFSET` | Stable |
| Deduplication | `DISTINCT`, `DISTINCT ON` | Stable |
| Aggregation | `count`, `sum`, `avg`, `min`, `max`, grouping and `HAVING` | Stable |
| Join syntax | Inner, left, right, full outer, cross, lateral, apply, semi, and anti forms with legality-preserving planning | Experimental |
| Subqueries | Implemented table, predicate (including EXISTS), lateral, and correlated forms; scalar expression subqueries are absent. | Experimental |
| Scalar expression subqueries | Absent; selected syntax, type, correlation and cardinality semantics are tracked in [#762](https://github.com/cntryl/cassie/issues/762). | Planned |
| Common table expressions | Non-recursive and recursive `WITH` | Experimental |
| Set operations | `UNION`, `UNION ALL`, `INTERSECT`, `EXCEPT` | Stable |
| Window functions | Ranking, offset, value functions, and documented row frames | Experimental |
| Views | Read-only views and nested views | Stable |
| User functions | Scalar UDFs with declared volatility | Experimental |
| Procedures | `CREATE PROCEDURE`, `CALL`, and `DROP PROCEDURE` for the narrow one-statement contract in [Limited Procedures and CALL](procedure-support.md); not a stored-procedure business-logic platform | Stable |
| Types and casts | Text, numeric, bool, timestamp, UUID, JSON, arrays, vectors, and supported casts | Experimental |
| Conditional scalar expressions | Selected `NULLIF`, `GREATEST` and `LEAST` over existing integer, FLOAT, TEXT and BOOLEAN families; see [Conditional expressions](conditional-expressions.md) for coercion, NULL, descriptor and exclusion rules. | Experimental |
| Reserved identity column | `_id` names the internal document identity and cannot be declared or dropped; a table may declare its own `id` column and query it as an ordinary column | Stable |

The [finite SQL type and wire contract](type-contract.md) records current logical
types, private wire identities, codec and metadata boundaries, exactness, NULL
meaning and explicit exclusions. Its qualification does not promote the broader
types/casts family.

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
| Tables and constraints | Table DDL, name-idempotent `CREATE TABLE IF NOT EXISTS`, defaults, unique, check, foreign key | Experimental |
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
| Column-batch analytics | Stable CBM2 batches provide deterministic automatic plain, constant, typed RLE, dictionary, 128-value frame-of-reference, FSST UTF-8 symbol streams, and ALP decimal-scaled float blocks, summary pruning, late materialization, encoded scans, exact filtered aggregate acceleration, and generation-fenced range copy-on-write maintenance. FSST uses manifest codec tag 6 and codec version 1 with at most 256 deterministic symbols and a bounded 64 KiB symbol table; selective projection validates the complete table, framing, indices, decoded sizes, and UTF-8 while materializing only selected values. ALP uses manifest codec tag 5 and codec version 1, encodes only finite bit-exact values, and leaves signed zero or non-exact values on plain encoding. Unsupported, stale, corrupt, unknown-version, or over-limit derived state falls back atomically to authoritative rows. Codec selection is not a SQL or configuration surface. | Stable |
| Graph traversal | Neighbor expansion and shortest paths | Experimental |

## Planning, Execution, and Interfaces

| Capability | Behavior | Status |
| --- | --- | --- |
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
