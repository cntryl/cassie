# PostgreSQL Compatibility

This document is the canonical contract for PostgreSQL wire protocol and client interoperability. SQL feature behavior and status live in [Feature Support](feature-support.md).

## Compatibility Goal

Cassie provides a PostgreSQL-like query interface for read-model workloads. It aims to work with common drivers and administration tools for documented workflows, without claiming PostgreSQL server, extension, catalog, transaction-isolation, or DDL parity.

## Connection and Authentication

- Pgwire is the primary query interface and defaults to `127.0.0.1:5432`.
- Startup negotiates protocol version, user, database, and supported parameters.
- Password authentication uses Cassie roles and stored password hashes.
- Invalid known and unknown users follow the same password-verification path. Process-local normalized-user and peer-IP token buckets bound authentication attempts; pgwire intentionally returns the same generic authentication failure when a limit is exhausted.
- Each authenticated connection is bound to one existing database.
- Passwordless bootstrap is limited to embedded use without a network listener. Pgwire and REST listener startup reject an empty bootstrap password or a persisted passwordless bootstrap role.
- The default bootstrap password is loopback-only. Non-loopback pgwire and REST listeners require a non-default credential plus Cassie-managed TLS unless `CASSIE_ALLOW_INSECURE_NON_LOOPBACK_LISTEN=1` explicitly permits a trusted private hop behind a TLS-terminating reverse proxy or load balancer. Plaintext listener traffic must not be exposed directly to an untrusted network.
- Connection admission is bounded and failures are reported using PostgreSQL-style error responses.
- Database access can be managed by an administrator with `GRANT CONNECT ON DATABASE database TO role` and `REVOKE CONNECT ON DATABASE database FROM role`. Cassie supports one database and one role per statement; grant options, multiple grantees, ownership, and table privileges are outside this contract. Grants and revocations are idempotent, administrators retain implicit access, and active sessions revalidate access at each statement.

## Database Image Authorization

Pgwire `BACKUP DATABASE ... TO STDOUT` and `RESTORE DATABASE ... FROM STDIN` are admin-only operations. All authenticated non-admin roles are read-only, and `GRANT CONNECT` authorizes connection to a database but never authorizes database-image export or import. The target database access check in the pgwire image path is defense-in-depth under this binary role model; it does not represent a separately reachable database-scoped backup or restore permission.

Cassie does not currently expose a named database-image capability or a non-admin, non-read-only network principal. That finer-grained capability is absent by product contract and its absence is not a regression. Adding one would require a separately specified role/session capability, authorization semantics for source and target databases, and a regression where the coarse role check and database-scoped permission deliberately disagree. The sessionless embedded database-image APIs are trusted host calls whose authorization remains the embedding application's responsibility.

## Session Model

- `current_user`, `current_database()`, `current_schema()`, `SHOW search_path`, and `SET search_path` reflect session state.
- `current_schema()` resolves `$user`, skips missing schemas, and returns SQL NULL when no search-path schema exists. It follows the same catalog-aware resolution as unqualified names.
- Session-mutating `set_config` calls execute for every session, including qualified calls and calls through stored views; execution-result caching never substitutes a returned row for the setting change.
- Startup parameters, `SET`, `SHOW`, `current_setting`, `set_config`, `pg_settings`, and `pg_show_all_settings()` share one validated settings contract.
- `set_config` accepts boolean values for `is_local`; true requests are rejected because transaction-local settings are unsupported, while false applies the setting to the session. Invalid boolean text and non-boolean values are errors.
- Mutable settings are `search_path`, `application_name`, and `client_min_messages`. Cassie validates fixed PostgreSQL-facing values for server and client encoding, date style, time zone, standard strings, integer datetimes, bytea output, extra float digits, and the advertised server version.
- Unsupported setting names and incompatible fixed values are errors; Cassie does not silently accept arbitrary PostgreSQL GUCs.
- Unqualified relations resolve through `search_path` inside the current database.
- A quoted relation identifier is one component: dots inside `"name.with.dots"` do not add schema or database qualifiers, and doubled quotes (`"a""b"`) decode to one quote in the component. SQL relation paths retain their component boundaries in the AST or in reversible escaped spelling, and the binder converts them to `IdentifierPath` before scope resolution.
- Canonical relation names use `database.schema.relation` order. Each component that contains a dot, a double quote, or whitespace is double-quoted in the canonical spelling, with embedded double quotes doubled. For example, the relation `"triage.dot"` in `public` in the `postgres` database is stored and exposed as `postgres.public."triage.dot"`. Components without those characters keep their existing spelling. Parsing and formatting this name must round-trip the same three component values.
- Midge continues to encode the canonical name as a framed key component under the current key layout; row data remains addressed by its numeric relation ID. A quoted single-component name that needs escaping does not use the unscoped raw-name fallback, so it cannot resolve to a schema-qualified relation with the same dotted spelling.
- `CREATE TABLE`, `CREATE VIEW`, `CREATE SEQUENCE`, `CREATE GRAPH`, `CREATE FUNCTION`, `CREATE PROCEDURE`, `CREATE ROLLUP` and `CREATE MATERIALIZED PROJECTION` fail with SQLSTATE `3F000` (`invalid_schema_name`) when the target schema does not exist, whether it is named explicitly (`CREATE TABLE ghost.t ...`) or reached through `search_path`. `public` always exists.
- Cross-database relation references are unsupported.
- Prepared statements and portals belong to one connection and are removed when closed or disconnected.
- Transactions accept Cassie's documented isolation behavior only; unsupported modes return `0A000`.

## Protocol Coverage

| Protocol surface | Contract |
| --- | --- |
| Simple query | Up to 256 supported SQL statements with row descriptions, incrementally emitted data rows, command completion, and ready state |
| Extended query | Parse, bind, describe, execute, close, flush, and sync |
| Parameters | Text and supported binary encodings with deterministic type validation; a parameter decodes to the same value in either format (text `DATE`/`TIME`/`TIMESTAMP`, `UUID` and `BYTEA` are canonicalized, `float4`/`numeric` decode as numbers, and `bool`/integer text accepts the PostgreSQL input spellings) |
| Prepared statements | Named and unnamed statements scoped to the connection |
| Portals | Named and unnamed portals with per-execute `max_rows`, suspension, resume, cumulative result and retained-memory limits, and cleanup |
| Cancellation | Backend key data plus PostgreSQL cancel requests using process ID and secret |
| Copy ingestion | Supported CSV `COPY FROM STDIN` workflow |

## Mutation and DDL Subset

- Upsert uses `INSERT ... ON CONFLICT`. `DO NOTHING` accepts an optional target; `DO UPDATE` requires an explicit primary-key, unique-constraint, or plain unique scalar-index target and supports existing-row expressions, `excluded.<column>`, parameters, a `WHERE` filter, and `RETURNING`.
- `CREATE TABLE IF NOT EXISTS` and `CREATE [UNIQUE] INDEX IF NOT EXISTS` are name-only no-ops. An existing name succeeds even when the requested definition differs, preserving the existing object and schema epoch. Without the clause, duplicates are errors.
- The index rule applies to Cassie's scalar, full-text, vector, column, hybrid, and time-series index kinds.
- Standalone `UPSERT`, `ON CONSTRAINT`, concurrent conflict arbitration, and partial or expression-index conflict inference are unsupported.
- `_id` is a reserved identifier naming Cassie's internal document identity. `CREATE TABLE` and `ALTER TABLE` reject a field named `_id`, and `ALTER TABLE DROP COLUMN _id` is an error. Cassie has no PostgreSQL system-column equivalent such as `ctid` or `oid`.
- A table may declare its own `id` column, including `id INT PRIMARY KEY`. It then behaves as an ordinary column in every statement, and drops like any other column. A table that declares no `id` resolves a bare `id` reference to the internal identity instead; that value is what `SELECT *` returns in its leading column. Applications that want PostgreSQL-portable behavior should declare `id` explicitly rather than relying on the implicit identity.
- `_id` is selectable, filterable, and sortable in SQL against a base table and always names the internal identity, whether or not the table declares its own `id`: `SELECT _id, id FROM t WHERE _id = $1 ORDER BY _id` returns both values. `ORDER BY <indexed column>, _id` keeps the ordered scalar-index read path with the identity as a deterministic tiebreaker; `ORDER BY <indexed column>, id` against a declared `id` sorts on that ordinary column instead. A derived relation (subquery, CTE, or set-operation branch) exposes only the columns it projects, so a bare `id` or `_id` that the body does not project is an unresolvable column reference rather than NULL.
- Adding a column named `id` with `ALTER TABLE ... ADD COLUMN id ...` changes what a bare `id` means for that table: references that previously resolved to the internal identity now resolve to the new column, which is NULL for every row written before the change. `_id` continues to name the identity. Dropping that column with `ALTER TABLE ... DROP COLUMN id` moves the table back to resolving bare `id` to the identity.
- A column `DEFAULT` can be a constant (optionally parenthesized, with a single-word `::type` cast such as `'draft'::varchar`), `nextval(...)`, or one of the volatile functions `now()`, `CURRENT_TIMESTAMP[(p)]`, `LOCALTIMESTAMP[(p)]`, `CURRENT_DATE` and `gen_random_uuid()`. Each volatile function is evaluated once per inserted row. When the DDL runs, a constant is coerced to the column's declared type, so `BOOLEAN DEFAULT 'f'` stores `false`. `CREATE TABLE`, `ADD COLUMN` and `ALTER COLUMN ... SET DEFAULT` reject a constant or function that the column type cannot hold, and they reject any other expression (for example `abs(-3)`) instead of storing its text.
- A `CHECK` constraint compares one column with a constant (`NULL`, `TRUE`, `FALSE`, a single-quoted string or a number). `CREATE TABLE` and `ALTER TABLE ... ADD CONSTRAINT` reject a check that compares with another column (bare or double-quoted) or any other expression. Before this, such a check was accepted and its right-hand side was stored as text. String constants in `CHECK` and `DEFAULT` decode `''` to one quote, as they do everywhere else.
- Trailing blanks are insignificant in a `CHAR(n)` value. Cassie stores and returns the value without them, so `'x'` and `'x  '` are the same value for equality with a stored value, `UNIQUE` constraints and unique indexes, and a value whose extra length is only trailing blanks fits the column (`'x  '` fits `CHAR(1)`). PostgreSQL behaves differently in two ways. It pads the value to `n` on output, while Cassie returns it without padding. It also compares a literal that has trailing blanks (`WHERE c = 'x  '`) as `CHAR`, while Cassie compares it as text.
- REST document endpoints report the same internal identity as `"id"` in their JSON body (for example `{"id": "<identity>"}` from a document write). That value is the SQL `_id` of the row, and on a table that declares no `id` column it is also what a SQL bare `id` returns. On a table that declares its own `id`, the REST `"id"` field is still the internal identity, not the declared column; read the declared column through SQL or a document field.

## Expression Semantics

- `ORDER BY <n>` with a bare integer sorts by the n-th output column of the select list (1-based), including after `GROUP BY` and after a set operation. A position outside the select list is an error (`ORDER BY position N is not in select list`). Positions are not supported when the select list contains `*` up to and including that position, or when they name an unaliased window function; both cases are rejected with a feature-not-supported error. An aggregate position behaves exactly like `ORDER BY <its alias>`.

- Column identifier components follow PostgreSQL case rules throughout DDL and query execution. Unquoted components fold to ASCII lowercase; delimited components preserve their exact spelling and match only that spelling. For example, `CREATE TABLE t (Email TEXT, "Status" TEXT)` declares `email` and `Status`: `SELECT EMAIL` resolves to `email`, `SELECT "Status"` resolves to `Status`, and `SELECT "Email"` is an undefined-column error. Delimited columns such as `"a"` and `"A"` may coexist and remain distinct across projection, predicates, grouping, ordering, DML, indexes, and constraints. Relation-name matching is outside this column-name contract.
- Cassie does not rewrite persisted schema metadata when this contract is introduced. A pre-existing mixed-case field name remains stored with that exact spelling and can be referenced by quoting it exactly; an unquoted reference folds to lowercase and may no longer resolve to that legacy spelling.
- Searched `CASE WHEN condition THEN value` and simple `CASE operand WHEN value THEN result` are supported, including nested expressions. Searched conditions must be Boolean; only `TRUE` selects a branch. Simple matching uses SQL equality, so `NULL` never matches. Only the selected result branch is evaluated, and an unmatched expression without `ELSE` returns `NULL`. Result types ignore `NULL` branches, widen compatible numeric branches, normalize compatible text branches, and reject incompatible families.
- `LIKE` follows PostgreSQL matching: it is case-sensitive, `%` matches any sequence of zero or more characters anywhere in the pattern, and `_` matches exactly one character. A backslash escapes the next pattern character, so `'a\%c'` matches the literal text `a%c`; a pattern that ends with an unpaired backslash is an error. The same rules apply to `LIKE` in `CHECK` constraints, where a malformed pattern is reported as a pattern error rather than treated as a failed check. The `ESCAPE` clause, `ILIKE`, and `NOT LIKE` are not supported; use `NOT (expr LIKE pattern)` for negation. `LIKE ... ESCAPE` is rejected with a feature-not-supported error rather than read as part of the pattern.
- The `||` concatenation operator is not supported and is rejected with a feature-not-supported error; use `concat()`. Note that `concat()` skips `NULL` arguments, whereas PostgreSQL's `||` returns `NULL` when either operand is `NULL`. A single-quoted string literal ends at its closing quote, so an expression such as `'a' || 'b'` or two adjacent literals is never collapsed into one string.
- `DATE`, `TIME`, and `TIMESTAMP` string input is validated and canonicalized by one shared parser (`crate::types::temporal`) used on write (so `RETURNING` reports the stored form), on `CAST`, by the pgwire text and binary parameter codecs, by `time_bucket`, by time-series index maintenance and reads, and by retention policy validation and enforcement. An unparseable string is rejected rather than stored as-is. The canonical stored `TIMESTAMP` form is fixed-width UTC with a trailing `Z` and six fractional digits (for example `2024-01-01T07:00:00.000000Z`), so plain text comparison and scalar-index key order are chronological. Rows written before the fixed-width form keep their variable-width text (for example `2024-01-01T07:00:00Z`) and are not rewritten; executor comparisons (`ORDER BY`, `WHERE` comparisons, equality, `MIN`/`MAX`, grouping, and joins) widen any string of the canonical `YYYY-MM-DDTHH:MM:SS[.f{1,6}]Z` shape to the fixed-width form first, so old and new rows compare by instant. That widening is applied by value shape, so a `TEXT` column holding strings of exactly that shape compares them the same way. Scalar indexes built before the fixed-width form keep their variable-width keys; rebuild them (`DROP INDEX` then `CREATE INDEX`) so index range scans and index-ordered reads match full scans for whole-second values written before the change. Rows written before temporal validation existed keep whatever string they already had.
- **Deviation:** Cassie has a single `TIMESTAMP` type and no separate `TIMESTAMP WITH TIME ZONE`/`timestamptz`. PostgreSQL's `timestamp` (without time zone) silently discards an explicit offset in its input (`'2024-01-01T09:00:00+02:00'::timestamp` is `2024-01-01 09:00:00`); Cassie instead converts the offset to UTC (`2024-01-01T07:00:00.000000Z`), which matches `timestamptz` semantics. Offset-free input is treated as already UTC.

## Boolean Typing

Cassie's supported Boolean contexts require a Boolean value or SQL NULL. This applies to SELECT and DML `WHERE`, `HAVING`, `JOIN ON`, `ON CONFLICT DO UPDATE WHERE`, `AND`, `OR`, `NOT`, and searched `CASE WHEN` conditions, including their supported CTE and subquery forms. Known incompatible types fail during binding, including on an empty source or with `LIMIT 0`; absent rows do not make a numeric or text predicate valid.

- A direct single-quoted SQL literal has no declared type until its context is resolved. At a Boolean context, or when compared with a known Boolean operand, valid Boolean text is interpreted as Boolean: `WHERE 'no'` filters out every row, and `flag = 'no'` compares `flag` with false. A typed TEXT expression such as `CAST('no' AS TEXT)`, a TEXT column, a parameter declared with OID 25, and an embedded `Value::String` remain text. Their spelling does not make them implicit Boolean predicates or Boolean comparison operands. Use an explicit `CAST(... AS BOOLEAN)` for a supported conversion.
- Boolean text input accepts `true`, `yes`, `on`, `1`, `false`, `no`, `off`, and `0`, with ASCII case-insensitive word matching and surrounding whitespace ignored. Unique word prefixes such as `t`, `ye`, `n`, and `of` are accepted; `o` is ambiguous and rejected. Supported Boolean casts, writes, defaults, arrays, COPY fields, and text Bind input use this vocabulary, while each adapter retains its own NULL and container rules. This follows the documented [PostgreSQL Boolean input vocabulary](https://www.postgresql.org/docs/18/datatype-boolean.html).
- Boolean operators use SQL three-valued logic: `TRUE OR NULL` is true, `FALSE AND NULL` is false, and `NOT NULL` is NULL. Predicates and searched CASE conditions select only true; false and NULL do not select a row or branch. Ordinary equality with NULL remains NULL.
- Unknown parameter OIDs 0 and 705 infer OID 16 at supported Boolean predicate sinks. Recognized concrete parameter types remain concrete, and conflicting repeated Boolean and numeric requirements are rejected. Describe and execution distinguish Boolean and text parameter families when reusing plans. Boolean contextual checks retain a declared OID 16 even when bound to NULL, including execution without a preceding Describe.
- Explicit numeric-to-Boolean casts retain Cassie's existing conversion: zero is false and nonzero is true. A numeric parameter explicitly cast to Boolean keeps its declared input OID and reports Boolean result OID 16. This does not make numeric operands implicit Boolean predicates or add PostgreSQL's complete cast or operator catalog.
- Boolean comparison literals in the existing one-column/constant `CHECK` shape are normalized before CREATE or ALTER publishes the constraint, including ALTER on an empty table. NULL retains the existing CHECK acceptance rule. General CHECK expressions remain outside the supported shape described above.
- Invalid Boolean types or contextual literals fail with the existing planner SQLSTATE `42601`; invalid text evaluated by an explicit Boolean cast uses `22000`; malformed Boolean Bind input uses `08P01`. These are Cassie's existing error categories, not a promise of PostgreSQL SQLSTATE parity for every conversion.

The CHAR, numeric-width, temporal, and other type boundaries in this document continue to apply. This finite Boolean contract does not extend the supported SQL syntax, transaction profile, or PostgreSQL catalog surface.

## Numeric Typing

The [finite SQL type and wire contract](type-contract.md) lists every current
logical type, its metadata, storage representation, supported codecs and
explicit ABI exclusions. Unknown result/catalog metadata uses OID 705, typlen
−2 and typmod −1; SQL NULL fields retain wire length −1.

Text OIDs 700 and 1700 are numeric input adapters. Unconstrained output whose
type remains ambiguous requires an explicit cast to a current type and otherwise
fails with `0A000` before result descriptors or rows. Supported constrained
assignments, predicates and independently fixed result types retain their numeric
input behavior. Binary codecs for these two input OIDs remain unsupported.

Scalar arrays use Cassie's private identities and bounded one-dimensional
framing. ARRAY(VECTOR) wire identities and distinct JSON document-null scalar
elements inside ARRAY(JSON) are excluded with `0A000`; SQL-null elements and
nonNULL nested JSON documents containing null remain supported. These boundaries
do not change stored types or introduce PostgreSQL standard-array or pgvector ABI.

Cassie has no `numeric`/`decimal` type, so exact arithmetic is carried in `int8` and everything
else in `float8`. The rules below are stable and deliberately chosen to stay close to PostgreSQL.

- An integer literal is an integer, not a float: `1` is `int4` (`int8` when it does not fit `int4`), while a literal with a decimal point or exponent such as `1.0` is `float8`. `int_column + 1.0` is therefore `float8`; PostgreSQL would yield `numeric`, which Cassie does not have.
- Integer arithmetic (`+`, `-`, `*`, `/`) over integer operands is exact and reported as `int8`, where PostgreSQL reports `int4` for `int4` operands. Mixing an integer with a float produces `float8`.
- `/` over two integers truncates toward zero, as in PostgreSQL: `7 / 2` is `3` and `-7 / 2` is `-3`. Division by zero is `22012`. There is no `%` modulo operator.
- An overflowing `int8` result raises `bigint out of range` with SQLSTATE `22003` instead of silently widening to a float. PostgreSQL promotes overflowing `int4` arithmetic to `bigint`; Cassie already computes in `int8`, so only `int8` overflow is reachable.
- `COUNT` returns `int8` in every position: on its own, nested in `COALESCE` or `CASE`, in a view's output schema, and through a CTE.
- `COALESCE` requires compatible result types, using the same common-type rules as `CASE`; incompatible boolean/text or boolean/numeric combinations fail before result rows are sent. Bound parameter types are checked at description and execution, including DML RETURNING.
- UUID and BYTEA casts return canonical spelling. SELECT, UPDATE, and DELETE predicates canonicalize typed string literals consistently, including IN and BETWEEN.
- `SUM` over an integer argument returns `int8` and errors on overflow rather than falling back to an inexact float; PostgreSQL returns `numeric` for `SUM(bigint)`. `SUM` over a float argument returns `float8`.
- `AVG` always returns `float8`, including for integer arguments, where PostgreSQL returns `numeric`.
- `MIN`/`MAX` keep their argument's declared type.
- `CASE` unifies its result branches like PostgreSQL: integer branches stay integer, an integer branch beside a float branch widens the whole expression (and its values) to `float8`, and mixing text with a numeric branch is an error.

A CTE reports the types of the query that defines it, in both `SELECT` results and a pre-execution
`Describe`. Column aliases on the CTE (`WITH totals (name, seen) AS ...`) rename the columns while
keeping those types, a later CTE sees the types of the ones before it, and a recursive CTE takes its
types from the anchor term.

## Errors and Cancellation

Cassie emits PostgreSQL error responses with SQLSTATE codes where a stable mapping exists. Syntax errors use `42601`, unsupported features use `0A000`, undefined objects use their PostgreSQL-family codes, query cancellation and deadlines use `57014`, resource-limit failures use `54000`, and connection admission uses `53300`.

SQL text is limited to 1 MiB, 100,000 lexical tokens, 128 levels of SQL nesting, and 128 nested block comments. The scanner is linear and ignores delimiters inside quoted strings, quoted identifiers, and comments. These SQL-text limits apply to direct and pgwire parsing; the generic 16 MiB frontend frame limit remains available to bind and COPY payloads. Pgwire backend frames are individually capped at 16 MiB, and a resource-limit error leaves a simple-query connection ready for its next query.

`COPY ... FROM STDIN` and `RESTORE DATABASE ... FROM STDIN` follow the PostgreSQL copy-in contract. Flush and Sync messages received during copy-in are ignored. A CSV `COPY` stream is applied as one atomic batch, so its buffered CopyData total is capped at 16 MiB per statement; exceeding it fails with `54000`. When the backend rejects a copy (a parse, bind, or authorization error, a CopyFail, an oversized stream, an unexpected message, or a rejected database image), it sends ErrorResponse and ReadyForQuery right away and silently drops any CopyData, CopyDone, or CopyFail the client is still sending, so the connection stays ready for the next query. Any such error inside an explicit transaction aborts it (ReadyForQuery status `E`). `BACKUP DATABASE ... TO STDOUT` announces a single text-format column in its CopyOutResponse.

Supported SELECT clause and predicate keywords, JOIN/ON spellings, window ordering/frame keywords, and INSERT/DELETE/ON CONFLICT keywords accept whitespace and SQL comments as token separators. Formatting does not rewrite quoted values, quoted identifiers, stored view queries, routine bodies, or the statement's `raw_sql`. Explicit `INNER JOIN`, `LEFT OUTER JOIN`, and `RIGHT OUTER JOIN` use the corresponding existing join semantics.

Inside LATERAL queries, inner-source names take precedence over correlated outer names. Outer values remain available to expression lookup without becoming fields of the inner `SELECT *`. LEFT/RIGHT/FULL joins NULL-extend the complete ordered source shape, including qualified aliases, even when a table, view, subquery or CTE is empty. LEFT LATERAL output width is independent of whether each left row finds an inner match.

Every DataRow carries exactly as many fields as the RowDescription sent for the same result, encoded with the type and format that description announced. Built-in `pg_catalog` and `information_schema` views use the same declared column types for Describe and execution. A result row whose width does not match its description is reported as an internal error (`XX000`) rather than sent as a malformed frame.

A successful startup emits backend process and secret data. A cancel request affects only the matching live backend. Incorrect or stale secrets do nothing. Cancellation is cooperative at bounded execution checkpoints and cleans up query and portal resources. A cancelled resume returns `57014` and no partial row page.

Portal `max_rows` controls one execute response; it does not reset Cassie's query limits. Result rows are counted cumulatively across resumes, and retained memory is shared across all live portals on the connection. An execute or bind that would exceed a cumulative limit returns `54000` atomically. Closing a portal or statement, rolling back, or disconnecting releases its state.

## Catalog Contract

Cassie supplies the PostgreSQL-like virtual catalog rows needed by supported clients. These views describe Cassie objects; they are not byte-for-byte PostgreSQL catalogs. Applications must not depend on undocumented catalog columns, OIDs, server settings, extensions, or system functions.

The stable named-client subset and its exact columns, lifecycle behavior, failure contract, and
retained client evidence are defined in [Catalog Support](catalog-support.md). Only
`information_schema.tables` and `information_schema.columns` are Stable; all other virtual catalogs
remain Experimental.

## Client Evidence

The planned versioned certification targets are pgAdmin 9.16 and DBeaver 26.1.3 using PostgreSQL JDBC 42.7.11. The target workflow covers password/TLS connection and reconnection; supported database, schema, table, view, column, index, constraint, role, function, and procedure navigation; supported object properties; and Query Tool SQL execution, results, transactions, cancellation, graphical plans, and safe primary-key-based grid edits. The targets are not certified by the current repository until the required traces and external workflows are published.

Certification requires normalized, secret-free traces and deterministic replay for startup, initialization, navigator expansion, properties, query actions, plans, and grid edits. Each trace records the client and driver version, upstream source revision, workflow step, SQL, protocol mode, expected columns, and expected row shape. A passing trace certifies only that recorded workflow.

PostgreSQL replication, extensions, foreign-data wrappers, triggers, dashboards, maintenance, debugger, and backup tooling are explicitly unsupported. GUI create/alter dialogs are outside this contract; supported DDL remains available through query tools. Cassie does not claim full PostgreSQL, pgAdmin, or DBeaver parity.

The Stable procedure subset is defined separately in [Limited Procedures and CALL](procedure-support.md). It covers one persisted Cassie SQL statement and a named `tokio-postgres` workflow, not PostgreSQL procedural languages or trigger behavior.

The repository currently keeps automated coverage for the native pgwire harness and `tokio-postgres`; external Prisma, psql, and SQLAlchemy probes are opt-in when separately provisioned. Version-pinned pgAdmin 9.16 and DBeaver 26.1.3 normalized trace-replay lanes are available and retain explicitly non-certifying manifests. The psycopg 3 and JDBC client lanes remain Planned/unverified, and pgAdmin/DBeaver remain uncertified until separately provisioned live desktop workflows retain exact-revision evidence. Client-version upgrades require refreshed traces and live smoke suites before a future certification claim changes.

The shared dispatch contract for these lanes is documented in [Compatibility Probe Contract](compatibility-probe-contract.md)
and implemented by the opt-in [Compatibility Probes workflow](../.github/workflows/compatibility-probes.yml).

## Intentional Differences

- No full PostgreSQL parity or extension ABI.
- No distributed or serializable transaction promise.
- No trigger or stored-procedure business-logic platform.
- No cross-database queries.
- Cassie-specific search, vector, graph, time-series, projection, and administrative features may use PostgreSQL-compatible syntax without promising PostgreSQL semantics beyond their documented behavior.

## Finite Query Engine Target

[Query Engine Target 1](query-engine-target.md) records the finite type, protocol,
SQL and exact-version client workflows; the
[ownership ledger](query-engine-invariant-ownership.md) records qualification
owners and blockers. PostgreSQL 18 is a comparison reference, not a newly certified
server/client target. The bounded Query/Sync transaction profile was selected
for [#755](https://github.com/cntryl/cassie/issues/755) in [the durable direction](https://github.com/cntryl/cassie/issues/755#issuecomment-5982444561).
Its state tables, independent-session/unnamed-object and extended-to-simple
interleaving probes remain pending. Current explicit transactions and unsupported
isolation/DDL/extended-COPY behavior remain the implementation baseline until
that selected contract is implemented and qualified. This foundation neither
claims runtime completion nor adds transactional DDL or stronger isolation.

Scalar expression subqueries are not implemented; implemented table/predicate/
lateral/correlated forms remain evaluation surfaces. Full PostgreSQL server,
internal catalog, extension and pgvector ABI parity remain unclaimed. External
driver, trace replay and actual live desktop evidence remain separate; new client
versions need new finite-workflow evidence.
