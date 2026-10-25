# PostgreSQL Compatibility

> **Current operations constraint:** GitHub Actions are disabled by user direction. Do not
> enable or dispatch archived client workflows until the user explicitly changes that direction.
> The workflow descriptions below record historical interfaces; they do not authorize a live run
> or establish current certification.

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

- `current_user`, `session_user`, `current_role`, `current_catalog`, `current_database()`, `current_schema()`, `pg_client_encoding()`, `SHOW search_path`, and `SET search_path` reflect session state. The selected identity helpers return PostgreSQL `name` identity (OID 19). `pg_client_encoding()` reads the validated session `client_encoding` value.
- `current_schema()` resolves `$user`, skips missing schemas, and returns SQL NULL when no search-path schema exists. It follows the same catalog-aware resolution as unqualified names.
- Session-mutating `set_config` calls execute for every session, including qualified calls and calls through stored views; execution-result caching never substitutes a returned row for the setting change.
- Startup parameters, `SET`, `SHOW`, `current_setting`, `set_config`, `pg_settings`, and `pg_show_all_settings()` share one validated settings contract.
- `set_config` accepts boolean values for `is_local`; true requests are rejected because transaction-local settings are unsupported, while false applies the setting to the session. Invalid boolean text and non-boolean values are errors.
- Mutable settings include `search_path`, `application_name`, `client_min_messages`, `TimeZone`, `DateStyle`, `IntervalStyle`, `default_transaction_read_only` and the timeout and `pg_dump` preamble settings below. Cassie validates fixed PostgreSQL-facing values for server and client encoding, standard strings, integer datetimes, bytea output, extra float digits, and the advertised server version.
- Unsupported setting names and incompatible fixed values are errors; Cassie does not silently accept arbitrary PostgreSQL GUCs.
- `SET`, `SET LOCAL`, `SET ... TO DEFAULT`, `RESET name`, `RESET ALL` and `SHOW ALL` cover every registered setting. A transaction rollback, a rollback to a savepoint and the end of a transaction (for `SET LOCAL`) restore the earlier values. Startup packets may name mutable settings directly or pass them in `options` as `-c name=value` or `--name=value`.
- `statement_timeout` is enforced as a statement deadline on the wire, REST and embedded paths and cancels with `57014`. Its value uses PostgreSQL's millisecond units (`ms`, `s`, `min`, `h`, `d`) and is shown with the largest exact unit (`1500ms`, `90s`, `1min`).
- `lock_timeout`, `idle_in_transaction_session_timeout`, `idle_session_timeout` and `transaction_timeout` are validated and shown like PostgreSQL but are compatibility-only: Cassie has no lock waits, idle-session reaper or transaction deadline for them to bound. `check_function_bodies`, `row_security` (no row-security policies exist), `xmloption` (`content` or `document`) and `default_table_access_method` (`heap` only) are likewise accepted with PostgreSQL validation and no execution effect. `standard_conforming_strings` accepts only `on`.
- `client_encoding` and `SET NAMES` accept `UTF8` and its aliases (`UTF-8`, `UNICODE`). Every other encoding name, including the other encodings PostgreSQL supports, is rejected with `22023` (`invalid value for parameter "client_encoding"`). Conversions to other encodings remain a pending obligation.
- `SET SCHEMA 'name'` sets a one-entry `search_path`; `SET SESSION CHARACTERISTICS AS TRANSACTION` sets the default read-only and deferrable modes.
- SQL `PREPARE name [(types)] AS statement`, `EXECUTE name [(arguments)]` and `DEALLOCATE [PREPARE] {name | ALL}` share one namespace with named protocol statements, as in PostgreSQL: a duplicate name fails with `42P05`, a missing name with `26000`, and `DEALLOCATE ALL` also drops protocol statements. `EXECUTE` coerces each argument to its declared parameter type. Prepared statements survive `ROLLBACK` and `RESET ALL`. `pg_prepared_statements` lists both kinds; its `parameter_types` and `result_types` columns are `text` in array spelling, and plan counters are zero.
- `DECLARE name [BINARY] [ASENSITIVE | INSENSITIVE] [[NO] SCROLL] CURSOR [{WITH | WITHOUT} HOLD] FOR query`, `FETCH` and `MOVE` in every direction (`NEXT`, `PRIOR`, `FIRST`, `LAST`, `ABSOLUTE n`, `RELATIVE n`, `n`, `ALL`, `FORWARD`/`BACKWARD [n | ALL]`) and `CLOSE {name | ALL}` follow PostgreSQL's positions and command tags. A cursor materializes its query when declared, so every cursor behaves as `INSENSITIVE`. Only a `SCROLL` cursor moves backward; PostgreSQL may also allow backward scans of a cursor declared without either option, while Cassie treats that default as `NO SCROLL` and rejects backward movement with `55000`. A cursor without `WITH HOLD` requires a transaction block (`25P01`) and closes when the transaction ends; a rollback, or a rollback to an earlier savepoint, closes cursors declared after it. `pg_cursors` lists open cursors. `WHERE CURRENT OF` is a pending obligation.
- `DISCARD ALL` closes cursors, resets every setting, deallocates SQL and protocol prepared statements and forgets sequence history; inside a transaction block it fails with `25001` and changes nothing. `DISCARD SEQUENCES` forgets `currval`/`lastval` without rewinding sequences. `DISCARD PLANS` and `DISCARD TEMP` succeed with nothing session-owned to drop: plans are cached by statement shape and settings, and Cassie has no session-scoped temporary relations.
- `EXPLAIN` accepts PostgreSQL's option list (`ANALYZE`, `VERBOSE`, `COSTS`, `SETTINGS`, `BUFFERS`, `WAL`, `TIMING`, `SUMMARY`, `GENERIC_PLAN`, `SERIALIZE`, `MEMORY`, `FORMAT`) with explicit boolean values and the legacy `EXPLAIN ANALYZE VERBOSE` form, validating option combinations with PostgreSQL's errors. Only `ANALYZE` changes execution; the plan is always Cassie's text plan, and `FORMAT JSON`, `YAML` and `XML` fail with `0A000`.
- Unqualified relations resolve through `search_path` inside the current database.
- A quoted relation identifier is one component: dots inside `"name.with.dots"` do not add schema or database qualifiers, and doubled quotes (`"a""b"`) decode to one quote in the component. SQL relation paths retain their component boundaries in the AST or in reversible escaped spelling, and the binder converts them to `IdentifierPath` before scope resolution.
- Canonical relation names use `database.schema.relation` order. Each component that contains a dot, a double quote, or whitespace is double-quoted in the canonical spelling, with embedded double quotes doubled. For example, the relation `"triage.dot"` in `public` in the `postgres` database is stored and exposed as `postgres.public."triage.dot"`. Components without those characters keep their existing spelling. Parsing and formatting this name must round-trip the same three component values.
- Midge continues to encode the canonical name as a framed key component under the current key layout; row data remains addressed by its numeric relation ID. A quoted single-component name that needs escaping does not use the unscoped raw-name fallback, so it cannot resolve to a schema-qualified relation with the same dotted spelling.
- `CREATE TABLE`, `CREATE VIEW`, `CREATE SEQUENCE`, `CREATE GRAPH`, `CREATE FUNCTION`, `CREATE PROCEDURE`, `CREATE ROLLUP` and `CREATE MATERIALIZED PROJECTION` fail with SQLSTATE `3F000` (`invalid_schema_name`) when the target schema does not exist, whether it is named explicitly (`CREATE TABLE ghost.t ...`) or reached through `search_path`. `public` always exists.
- Cross-database relation references are unsupported.
- Named prepared statements belong to one connection and share its SQL `PREPARE` namespace. Portals belong to their transaction or preparation segment and close at commit/rollback, idle Sync, or disconnect; paging across Sync requires explicit BEGIN. Accepted Simple Query resets the unnamed statement and portal. See the [wire transaction contract](pgwire-transaction-contract.md).
- Transactions accept Cassie's documented isolation behavior only; unsupported modes return `0A000`.

## Protocol Coverage

| Protocol surface | Contract |
| --- | --- |
| Simple query | Up to 256 supported SQL statements with row descriptions, incrementally emitted data rows, command completion, and ready state |
| Extended query | Parse, bind, describe, execute, close, flush, and sync |
| Parameters | Type-specific text and supported binary encodings with deterministic validation; original scalar identities retain their codecs, while registered native extended types (including `real`, exact `numeric`, temporal, network and bit-string families) use their selected family codecs. See the [type contract](type-contract.md); registration does not imply support for every cast or array form. |
| Prepared statements | Named and unnamed statements scoped to the connection |
| Portals | Named and unnamed portals with per-execute `max_rows`, suspension, resume, cumulative result and retained-memory limits, and cleanup |
| Cancellation | Backend key data plus PostgreSQL cancel requests using process ID and secret |
| Copy ingestion | Supported CSV `COPY FROM STDIN` workflow |

## Mutation and DDL Subset

- Upsert uses `INSERT ... ON CONFLICT`. `DO NOTHING` accepts an optional target; `DO UPDATE` requires an explicit primary-key, unique-constraint, or plain unique scalar-index target and supports existing-row expressions, `excluded.<column>`, parameters, a `WHERE` filter, and `RETURNING`.
- Selected INSERT default cells use the existing column-default allocator: `VALUES (DEFAULT, ...)` may mix defaults and explicit values in each row, including reordered target columns. Explicit SQL NULL suppresses a default and remains distinct from JSON null. Identity ALWAYS accepts DEFAULT; USER overriding allocates identity values rather than using supplied values. Defaults are prepared once before an upsert waits and are not reapplied for EXCLUDED. This finite increment adds no stored row type or sequence format. [Cell and identity probes](../tests/sql_mutations/insert/default_cells.rs) cover mixed rows, explicit NULL, serial allocation, identity overriding, tuple assignments and deferred conflict intents. [Owner-wait probes](../tests/compatibility/lifecycle/mutations/upsert_concurrency.rs) cover both omitted and explicit defaults plus JSON SQL NULL across commit and rollback. Existing VALUES, constraint and identity paths supply the prerequisites; broader relation forms in #800 and mutation qualification in #769 remain separate obligations.
- `CREATE TABLE IF NOT EXISTS` and `CREATE [UNIQUE] INDEX IF NOT EXISTS` are name-only no-ops. An existing name succeeds even when the requested definition differs, preserving the existing object and schema epoch. Without the clause, duplicates are errors.
- `ALTER TABLE ... ADD COLUMN IF NOT EXISTS` is also name-only: an existing field keeps its current type, default, and values when the clause skips it. Adding a field with a literal default writes the coerced value to historical rows, including rows staged earlier in an explicit transaction, and uses that default for later inserts. Reads in that transaction see the new field and its backfilled values before commit. Focused evidence: [`should_add_column_if_not_exists_without_replacing_existing_default`](../tests/compatibility/lifecycle/ddl/ddl_extensions/semantics.rs), [`should_rollback_alter_table_column_addition`](../tests/compatibility/lifecycle/ddl/ddl_extensions/transactions.rs), [`should_apply_added_literal_default_to_rows_staged_before_alter_commit`](../tests/compatibility/lifecycle/ddl/ddl_extensions/transactions.rs), [`should_preserve_existing_column_default_when_add_if_not_exists_runs_in_transaction`](../tests/compatibility/lifecycle/ddl/ddl_extensions/transactions.rs), [`should_commit_added_literal_default_backfill_without_other_staged_writes`](../tests/compatibility/lifecycle/ddl/ddl_extensions/transactions.rs), [`should_replay_add_column_derived_state_after_schema_commit_interruption`](../tests/catalog_security/ddl/schema_operation_recovery.rs), and [`should_resume_add_column_default_backfill_after_batch_interruption`](../tests/catalog_security/ddl/schema_operation_recovery.rs). Legacy pending field-add record decoding is covered by [`should_decode_pending_field_add_without_default_from_legacy_record`](../src/midge/adapter/schema_ops_tests.rs).
- The index rule applies to Cassie's scalar, full-text, vector, column, hybrid, and time-series index kinds.
- Standalone `UPSERT` is unsupported. Selected scalar-index inference supports the stored expression keys and structurally matching partial-index predicate. A partial unique index cannot satisfy a plain column target without that predicate; an eligible full unique index can. An unmatched target fails with `42P10` before rows change. This is finite inference support, not general predicate implication. See the [inference probes](../tests/compatibility/lifecycle/mutations/partial_index_predicate_inference.rs) and [expression-index probe](../tests/sql_mutations/operators/conflict_inference/parameters.rs). Concurrent conflict arbitration follows the bounded [transaction ownership contract](pgwire-transaction-contract.md#selected-read-committed-unique-owner-wait-slice).
- `_id` is a reserved identifier naming Cassie's internal document identity. `CREATE TABLE` and `ALTER TABLE` reject a field named `_id`, and `ALTER TABLE DROP COLUMN _id` is an error. Cassie has no PostgreSQL system-column equivalent such as `ctid` or `oid`.
- A table may declare its own `id` column, including `id INT PRIMARY KEY`. It then behaves as an ordinary column in every statement, and drops like any other column. A table that declares no `id` resolves a bare `id` reference to the internal identity instead; that value is what `SELECT *` returns in its leading column. Applications that want PostgreSQL-portable behavior should declare `id` explicitly rather than relying on the implicit identity.
- `_id` is selectable, filterable, and sortable in SQL against a base table and always names the internal identity, whether or not the table declares its own `id`: `SELECT _id, id FROM t WHERE _id = $1 ORDER BY _id` returns both values. `ORDER BY <indexed column>, _id` keeps the ordered scalar-index read path with the identity as a deterministic tiebreaker; `ORDER BY <indexed column>, id` against a declared `id` sorts on that ordinary column instead. A derived relation (subquery, CTE, or set-operation branch) exposes only the columns it projects, so a bare `id` or `_id` that the body does not project is an unresolvable column reference rather than NULL. In a select list, `relation.*` expands only columns visible through that source namespace, including for CTEs and derived relations; an unknown qualifier fails binding.
- Adding a column named `id` with `ALTER TABLE ... ADD COLUMN id ...` changes what a bare `id` means for that table: references that previously resolved to the internal identity now resolve to the new column. Without a default, rows written before the change read as NULL; a literal default is applied to those rows. `_id` continues to name the identity. Dropping that column with `ALTER TABLE ... DROP COLUMN id` moves the table back to resolving bare `id` to the identity.
- A column `DEFAULT` can be a constant (optionally parenthesized, with a single-word `::type` cast such as `'draft'::varchar`), `nextval(...)`, or one of the volatile functions `now()`, `CURRENT_TIMESTAMP[(p)]`, `LOCALTIMESTAMP[(p)]`, `CURRENT_DATE` and `gen_random_uuid()`. Each volatile function is evaluated once per inserted row. When the DDL runs, a constant is coerced to the column's declared type, so `BOOLEAN DEFAULT 'f'` stores `false`. `CREATE TABLE`, `ADD COLUMN` and `ALTER COLUMN ... SET DEFAULT` reject a constant or function that the column type cannot hold, and they reject any other expression (for example `abs(-3)`) instead of storing its text.
- `CREATE TABLE ... LIKE ... INCLUDING IDENTITY` copies a supported identity generation mode and sequence options into a new target-owned sequence, independent of the source. The final `INCLUDING` or `EXCLUDING IDENTITY` action wins, and `INCLUDING DEFAULTS` alone does not copy identity. Evidence: [`should_copy_identity_sequence_independently_with_like_including_identity`](../tests/compatibility/lifecycle/sequences/ddl_extensions/semantics.rs) and [`should_not_copy_identity_when_like_includes_only_defaults`](../tests/compatibility/lifecycle/tables/explicit_table_clone.rs) and [`should_exclude_identity_when_a_later_like_action_excludes_identity`](../tests/compatibility/lifecycle/sequences/ddl_extensions/semantics.rs).
- A `CHECK` constraint compares one column with a constant (`NULL`, `TRUE`, `FALSE`, a single-quoted string or a number). Cassie's current profile supports at most one CHECK per field; another inline or table-level CHECK on that field rejects with SQLSTATE `0A000`. `CREATE TABLE` and `ALTER TABLE ... ADD CONSTRAINT` reject a check that compares with another column (bare or double-quoted) or any other expression. Before this, such a check was accepted and its right-hand side was stored as text. String constants in `CHECK` and `DEFAULT` decode `''` to one quote, as they do everywhere else.
- Trailing blanks are insignificant in a `CHAR(n)` value. Cassie stores and returns the value without them, so `'x'` and `'x  '` are the same value for equality with a stored value, `UNIQUE` constraints and unique indexes, and a value whose extra length is only trailing blanks fits the column (`'x  '` fits `CHAR(1)`). PostgreSQL behaves differently in two ways. It pads the value to `n` on output, while Cassie returns it without padding. It also compares a literal that has trailing blanks (`WHERE c = 'x  '`) as `CHAR`, while Cassie compares it as text.
- REST document endpoints report the same internal identity as `"id"` in their JSON body (for example `{"id": "<identity>"}` from a document write). That value is the SQL `_id` of the row, and on a table that declares no `id` column it is also what a SQL bare `id` returns. On a table that declares its own `id`, the REST `"id"` field is still the internal identity, not the declared column; read the declared column through SQL or a document field.

## Expression Semantics

- `ORDER BY <n>` with a bare integer sorts by the n-th output column of the select list (1-based), including after `GROUP BY` and after a set operation. A position outside the select list is an error (`ORDER BY position N is not in select list`). Positions are not supported when the select list contains `*` up to and including that position, or when they name an unaliased window function; both cases are rejected with a feature-not-supported error. An aggregate position behaves exactly like `ORDER BY <its alias>`.

- Column identifier components follow PostgreSQL case rules throughout DDL and query execution. Unquoted components fold to ASCII lowercase; delimited components preserve their exact spelling and match only that spelling. For example, `CREATE TABLE t (Email TEXT, "Status" TEXT)` declares `email` and `Status`: `SELECT EMAIL` resolves to `email`, `SELECT "Status"` resolves to `Status`, and `SELECT "Email"` is an undefined-column error. Delimited columns such as `"a"` and `"A"` may coexist and remain distinct across projection, predicates, grouping, ordering, DML, indexes, and constraints. Relation-name matching is outside this column-name contract.
- Cassie does not rewrite persisted schema metadata when this contract is introduced. A pre-existing mixed-case field name remains stored with that exact spelling and can be referenced by quoting it exactly; an unquoted reference folds to lowercase and may no longer resolve to that legacy spelling.
- Searched `CASE WHEN condition THEN value` and simple `CASE operand WHEN value THEN result` are supported, including nested expressions. Searched conditions must be Boolean; only `TRUE` selects a branch. Simple matching uses SQL equality, so `NULL` never matches. Only the selected result branch is evaluated, and an unmatched expression without `ELSE` returns `NULL`. Result types ignore `NULL` branches, widen compatible numeric branches, normalize compatible text branches, and reject incompatible families.
- `LIKE` follows PostgreSQL matching: it is case-sensitive, `%` matches any sequence of zero or more characters anywhere in the pattern, and `_` matches exactly one character. A backslash escapes the next pattern character, so `'a\%c'` matches the literal text `a%c`; a pattern that ends with an unpaired backslash is an error. The same rules apply to `LIKE` in `CHECK` constraints, where a malformed pattern is reported as a pattern error rather than treated as a failed check. The `ESCAPE` clause, `ILIKE`, and `NOT LIKE` are not supported; use `NOT (expr LIKE pattern)` for negation. `LIKE ... ESCAPE` is rejected with a feature-not-supported error rather than read as part of the pattern.
- The `||` concatenation operator is not supported and is rejected with a feature-not-supported error; use `concat()`. Note that `concat()` skips `NULL` arguments, whereas PostgreSQL's `||` returns `NULL` when either operand is `NULL`. A single-quoted string literal ends at its closing quote, so an expression such as `'a' || 'b'` or two adjacent literals is never collapsed into one string.
- Date/time values use native temporal parsing and carriers. `timestamp` is a timezone-free calendar value; `timestamptz` is a separate OID and instant value. Offset-free `timestamptz` input uses the session `TimeZone`, and its text output follows that setting. `time with time zone` and `interval` are also distinct native types. Precision-qualified temporal types retain their declared precision. [Extended Types](extended-types.md) owns the selected calendar, timezone, formatting, binary and clock-function boundaries; the earlier single-UTC-timestamp deviation is historical.

## Boolean Typing

Cassie's supported Boolean contexts require a Boolean value or SQL NULL. This applies to SELECT and DML `WHERE`, `HAVING`, `JOIN ON`, `ON CONFLICT DO UPDATE WHERE`, `AND`, `OR`, `NOT`, and searched `CASE WHEN` conditions, including their supported CTE and subquery forms. Known incompatible types fail during binding, including on an empty source or with `LIMIT 0`; absent rows do not make a numeric or text predicate valid.

- A direct single-quoted SQL literal has no declared type until its context is resolved. At a Boolean context, or when compared with a known Boolean operand, valid Boolean text is interpreted as Boolean: `WHERE 'no'` filters out every row, and `flag = 'no'` compares `flag` with false. A typed TEXT expression such as `CAST('no' AS TEXT)`, a TEXT column, a parameter declared with OID 25, and an embedded `Value::String` remain text. Their spelling does not make them implicit Boolean predicates or Boolean comparison operands. Use an explicit `CAST(... AS BOOLEAN)` for a supported conversion.
- Boolean text input accepts `true`, `yes`, `on`, `1`, `false`, `no`, `off`, and `0`, with ASCII case-insensitive word matching and surrounding whitespace ignored. Unique word prefixes such as `t`, `ye`, `n`, and `of` are accepted; `o` is ambiguous and rejected. Supported Boolean casts, writes, defaults, arrays, COPY fields, and text Bind input use this vocabulary, while each adapter retains its own NULL and container rules. This follows the documented [PostgreSQL Boolean input vocabulary](https://www.postgresql.org/docs/18/datatype-boolean.html).
- Boolean operators use SQL three-valued logic: `TRUE OR NULL` is true, `FALSE AND NULL` is false, and `NOT NULL` is NULL. Predicates and searched CASE conditions select only true; false and NULL do not select a row or branch. Ordinary equality with NULL remains NULL.
- Unknown parameter OIDs 0 and 705 infer OID 16 at supported Boolean predicate sinks. Recognized concrete parameter types remain concrete, and conflicting repeated Boolean and numeric requirements are rejected. Describe and execution distinguish Boolean and text parameter families when reusing plans. Boolean contextual checks retain a declared OID 16 even when bound to NULL, including execution without a preceding Describe.
- Explicit numeric-to-Boolean casts retain Cassie's existing conversion: zero is false and nonzero is true. A numeric parameter explicitly cast to Boolean keeps its declared input OID and reports Boolean result OID 16. This does not make numeric operands implicit Boolean predicates or add PostgreSQL's complete cast or operator catalog.
- Boolean comparison literals in the existing one-column/constant `CHECK` shape are normalized before CREATE or ALTER publishes the constraint, including ALTER on an empty table. NULL retains the existing CHECK acceptance rule. Column CHECK predicates that reference a sibling field, `NOT VALID` and `NOT ENFORCED` attributes, CHECK `VALIDATE CONSTRAINT` and `RENAME CONSTRAINT` operations, and general CHECK expressions are rejected with SQLSTATE `0A000` outside the supported shape described above.
- Invalid Boolean types or contextual literals fail with the existing planner SQLSTATE `42601`; invalid text evaluated by an explicit Boolean cast uses `22000`; malformed Boolean Bind input uses `08P01`. These are Cassie's existing error categories, not a promise of PostgreSQL SQLSTATE parity for every conversion.

The CHAR, numeric-width, temporal, and other type boundaries in this document continue to apply. This finite Boolean contract does not extend the supported SQL syntax, transaction profile, or PostgreSQL catalog surface.

## Function parameter inference

Unknown wire parameter OIDs 0 and 705 resolve to TEXT for `lower($1)`,
`lower(COALESCE($1, NULL))`, `COALESCE(lower($1), CAST(NULL AS TEXT))` and
`COALESCE($1, NULL)`. A BOOLEAN, INT or BIGINT typed NULL argument supplies
the corresponding common type for an otherwise unknown COALESCE input.
`COALESCE($1, TRUE)` uses BOOLEAN, including when its result is a predicate.
These selected paths retain matching declared input identities and qualify
NULL values, Statement/Portal descriptors and text/binary results.

SQL-defined functions infer these primitive parameter types from declared
arguments. A BOOLEAN argument can therefore retain OID 16 when the function
returns TEXT. The lookup follows the session database and search path, including
schema-qualified references, without copying stored function bodies. New lookup
scratch shares transient Parse admission: exhaustion reports `53200`, and
cancellation/deadline checks retain `57014`. Retained prepared metadata and
registered CancelRequest during Parse remain separate open obligations.

The existing public Rust inference facade preserves explicitly supplied OIDs,
including zero; wire Parse normalizes zero before inference. This compatibility
control does not qualify invalid overloads or argument types. A generic UNKNOWN
finalizer, complete callable/operator coercion, occurrence-order inference and
the remaining Boolean error-phase/descriptor-name differences were unqualified
at that accepted foundation. Issue #904 is a closed historical parent; active
ownership and release scope follow the current roadmap and live issue criteria.
The accepted [PR #935](https://github.com/cntryl/cassie/pull/935) evidence covers
156 selected text/binary function paths; at that merged revision the 14
known-Boolean controls retain ten differences and four matches. That foundation does not qualify every
callable, overload, operator, or error phase.

## Primitive SELECT occurrence resolution

The [selected occurrence contract](https://github.com/cntryl/cassie/issues/904#issuecomment-6115604094),
recorded under the now-closed #904 parent, covers the following finite primitive
SELECT behavior. Its PR acceptance record
binds focused, native and complete-gate evidence to the tested revision. It does
not qualify unlisted callables, overloads, operators, or error phases; those
require current roadmap scope and issue criteria.

The private fold gives each actual parameter use a stable ordinal and creation
type. Coercion changes that use and the shared slot without retagging earlier
uses. In covered SELECT queries, CTE/FROM inputs precede targets; Boolean
arguments transform and coerce one at a time; OFFSET precedes LIMIT; and only
eligible direct UNKNOWN target outputs finish as TEXT after the clauses.
For example, `SELECT $1 AS flag WHERE $1` with OID 0/705 must fail Parse with
`42P08`, while declared OID 16 retains its input identity, including typed NULL.
`SELECT $1 AS value WHERE FALSE` with unknown input must describe TEXT output.
A qualified declared/cast/metadata-known TEXT Boolean operand must fail with
`42804`; a direct string literal still uses contextual Boolean literal coercion.
The new diagnostics do not replace every existing planner `42601` category.

Column types for this fold require an exact namespace match or a verified
single-source qualifier. Missing, nonlocal or unrepresented direct-JOIN metadata
continues through the established binder rather than borrowing another source
column's unqualified type. Independent selected parameter checks remain active;
complete joined and outer input-signature resolution remains outside this
finite qualification.

Parse, selected typed execution, Describe and EXPLAIN share this checked
private result before pagination, caches or early EXPLAIN. Public Rust Vec/Option
signatures preserve supplied zero, and untyped embedded NULL remains distinct
from a wire UNKNOWN declaration. Slots inferred by nonselected operators retain
their existing error path. Arithmetic results and CASE remain outside the new
Boolean diagnostics, with existing nonprimitive CASE identities preserved. New occurrence, phase, target and summary
buffers are admitted against the same query controls, including growth overlap.
Existing catalog/field discovery remains its separate input-allocation boundary.
Rejected metadata releases its scratch; admitted semantic-error formatting and
frames retain their owner through send retirement. Only the transient OID-vector
owner survives successful metadata handoff; retained prepared metadata remains
open.

No unused-slot `42P18` policy or statement-wide UNKNOWN finalizer is selected.
Wider callable/operator metadata, nonselected DML/RETURNING transformations,
descriptor-name differences and full resource/restart obligations remain open.
The original .85/.72 acceptance records are historical. This increment does not
qualify unrelated behavior or complete the selected SQL-profile and workload
readiness owners #848 and #849. Issue #904 is closed; its historical acceptance
record does not define current backlog status. Issues #791–#793 were closed as
superseded.

## Numeric and temporal type boundaries

`REAL` (OID 700) is a native float4 family. `NUMERIC`/`DECIMAL` (OID 1700) is
an exact decimal family with native text/binary codecs, declared precision and
scale, arithmetic and selected aggregate/result-type rules. It is not a text
input adapter or an alias for float8. `FLOAT`/`DOUBLE PRECISION` retain float8
identity (OID 701). Exact current identities and row-blob behavior are listed
in the [Type Contract](type-contract.md); family semantics and limits live in
[Extended Types](extended-types.md) and [Feature Support](feature-support.md).

Integer arithmetic, remainder and bitwise results retain the resolved native
integer width: INT2 (OID 21), INT4 (OID 23), or INT8 (OID 20). Mixed integer
operands select the widest operand type; shifts retain the left operand type.
Result descriptors and binary payload widths agree with that selected type.
Each operation checks overflow at its resolved width. This replaces the former
BIGINT descriptor widening for non-shift expressions, as selected on 2026-10-24
for predictable pgwire/client behavior. Declared columns, stored values and
existing integer row encodings retain their identities.

Integer/float and integer/numeric comparison behavior follows the selected
operator or aggregate contract for that position. The
[mixed numeric comparison contract](mixed-numeric-comparison-contract.md)
records its finite operator-input rules and historical #904 provenance; issue
#904 is closed and is not a current delivery dependency. Remaining query, wire
and workload qualification is owned by current milestone issues. Integer,
NUMERIC and FLOAT8 result types must not be inferred from one another's carriers.

Scalar arrays retain Cassie's private identities and bounded framing. Element
families, typmods and text/binary behavior are finite; `ARRAY(VECTOR)` and
unlisted array forms are not implied by scalar type support. SQL NULL and JSON
document null retain their documented distinct meaning.

A CTE reports the types of the query that defines it, in both `SELECT` results and a pre-execution
`Describe`. Column aliases on the CTE (`WITH totals (name, seen) AS ...`) rename the columns while
keeping those types, a later CTE sees the types of the ones before it, and a recursive CTE takes its
types from the anchor term. Recursive `SEARCH DEPTH FIRST` and `SEARCH BREADTH FIRST` add the named
sequence column, and `CYCLE` adds the named mark and path columns; the direct generated outputs use
PostgreSQL RECORD/RECORD[] OIDs with Cassie's text value carrier. This does not provide general
composite/record field operations or binary codecs; those remain scoped to [#828](https://github.com/cntryl/cassie/issues/828).
Custom CYCLE mark/default literal constants are coerced to their common type for text, boolean,
integer, and NUMERIC values; a NULL-only pair resolves to text. The parser rejects casts and other
expressions in these positions, matching PostgreSQL's literal-only grammar.
Cycle keys require equality support; JSON keys and arrays containing JSON are rejected during
binding, matching PostgreSQL's missing equality operator for those types.

Explicit `MATERIALIZED` CTE results are shared across references. `NOT MATERIALIZED` expands eligible
side-effect-free simple queries; volatile CTEs retain one shared evaluation, and recursive CTEs keep
their working-row behavior. Parameter inference inside a materialized CTE is preserved, and retained
query memory is released when its result is dropped or a consumer fails. Nested expansion and routine
volatility that cannot be resolved remain deterministic rejection boundaries. Focused evidence is in
[`ctes.rs`](../tests/sql_queries/ctes/ctes.rs).

## Errors and Cancellation

Cassie emits PostgreSQL error responses with SQLSTATE codes where a stable mapping exists. Syntax errors use `42601`, unsupported features use `0A000`, undefined objects use their PostgreSQL-family codes, query cancellation and deadlines use `57014`, resource-limit failures use `54000`, and connection admission uses `53300`.

SQL text is limited to 1 MiB, 100,000 lexical tokens, 128 levels of SQL nesting, and 128 nested block comments. The scanner is linear and ignores delimiters inside quoted strings, quoted identifiers, and comments. These SQL-text limits apply to direct and pgwire parsing; the generic 16 MiB frontend frame limit remains available to bind and COPY payloads. Pgwire backend frames are individually capped at 16 MiB, and a resource-limit error leaves a simple-query connection ready for its next query.

`COPY ... FROM STDIN` and `RESTORE DATABASE ... FROM STDIN` follow the PostgreSQL copy-in contract. CSV `COPY FROM STDIN` supports `FORCE_NULL` and `FORCE_NOT_NULL` with `*` or a column list, including the implicit all-column target when the COPY column list is omitted. A force list that repeats a column fails with `42701`. An unquoted null marker remains SQL NULL unless its column is forced not-null; a quoted marker is text unless its column is forced null, and FORCE_NULL takes precedence when both options name a column. Force targets are checked before CopyInResponse: an existing but uncopied column fails with `42P10`, and an unknown column with `42703`. Explicit SQL NULL for a JSON/JSONB COPY field remains NULL even when that field has a default; omitted columns still receive defaults. A CSV `COPY` stream is applied as one atomic batch, so its buffered CopyData total is capped at 16 MiB per statement; exceeding it fails with `54000`. When the backend rejects a copy (a parse, bind, or authorization error, a CopyFail, an oversized stream, an unexpected message, or a rejected database image), it sends ErrorResponse and ReadyForQuery right away and silently drops any CopyData, CopyDone, or CopyFail the client is still sending, so the connection stays ready for the next query. Any such error inside an explicit transaction aborts it (ReadyForQuery status `E`). `BACKUP DATABASE ... TO STDOUT` announces a single text-format column in its CopyOutResponse.

Supported SELECT clause and predicate keywords, JOIN/ON spellings, window ordering/frame keywords, and INSERT/DELETE/ON CONFLICT keywords accept whitespace and SQL comments as token separators. Formatting does not rewrite quoted values, quoted identifiers, stored view queries, routine bodies, or the statement's `raw_sql`. Explicit `INNER JOIN`, `LEFT OUTER JOIN`, and `RIGHT OUTER JOIN` use the corresponding existing join semantics.

An explicit INSERT destination column list permits only whitespace or SQL comments before its VALUES or SELECT source. Any remaining destination-tail text, including a WITH prefix or arbitrary tokens, must fail before catalog lookup or writes with `INSERT column list is malformed` (`42601`). This complete-consumption boundary retains the existing finite INSERT grammar; [#875](https://github.com/cntryl/cassie/issues/875) records its regression evidence.

Inside LATERAL queries, inner-source names take precedence over correlated outer names. Outer values remain available to expression lookup without becoming fields of the inner `SELECT *`. LEFT/RIGHT/FULL joins NULL-extend the complete ordered source shape, including qualified aliases, even when a table, view, subquery or CTE is empty. LEFT LATERAL output width is independent of whether each left row finds an inner match.

Every DataRow carries exactly as many fields as the RowDescription sent for the same result, encoded with the type and format that description announced. Built-in `pg_catalog` and `information_schema` views use the same declared column types for Describe and execution. A result row whose width does not match its description is reported as an internal error (`XX000`) rather than sent as a malformed frame.

`VERIFY PROJECTION` shares its ten-column definition between execution and pre-execution Describe. Statement Describe uses text format; portal Describe uses the requested text or binary result format and matches the subsequent execution result.

`DIFF PROJECTION`, `COMPARE PROJECTION`, `PLAN REPAIR PROJECTION` and
`REPAIR PROJECTION` use their execution-owned fixed result schemas for statement
and portal Describe. Statement Describe uses text format; portal Describe uses
the requested text or binary result format, and both match the following
execution result. Their ordered schemas are:

- DIFF: `row_id TEXT`, `change TEXT`, `left_digest TEXT`, `right_digest TEXT`,
  `state TEXT`, `next_cursor TEXT`, `complete BOOLEAN`.
- COMPARE: `target TEXT`, `state TEXT`, `root_digest TEXT`,
  `manifest_digest TEXT`, `mismatches INT`, `report_id TEXT`.
- PLAN REPAIR and REPAIR: `state TEXT`, `projection_name TEXT`, `scope TEXT`,
  `target_collection TEXT`, `action TEXT`, `executable BOOLEAN`,
  `verification_required TEXT`, `post_verification_state TEXT`, `report_id TEXT`,
  `affected_objects TEXT`, `last_error TEXT`.

A successful startup emits backend process and secret data. A cancel request affects only the matching live backend. Incorrect or stale secrets do nothing. Cancellation is cooperative at bounded execution checkpoints and cleans up query and portal resources. A canceled resume returns `57014` and no partial row page.

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

PostgreSQL replication, foreign-data wrappers, triggers, dashboards, maintenance, debugger, and backup tooling are explicitly unsupported. Cassie does not load native PostgreSQL extension libraries or claim extension ABI parity; the only planned bounded extension shims are the `vector` and `pgcrypto` behaviors in [#843](https://github.com/cntryl/cassie/issues/843), with vector SQL behavior owned by [#844](https://github.com/cntryl/cassie/issues/844). GUI create/alter dialogs are outside this contract; supported DDL remains available through query tools. Cassie does not claim full PostgreSQL, pgAdmin, or DBeaver parity.

The Stable procedure subset is defined separately in [Limited Procedures and CALL](procedure-support.md). It covers one persisted Cassie SQL statement and a named `tokio-postgres` workflow, not PostgreSQL procedural languages or trigger behavior.

The repository currently keeps automated coverage for the native pgwire harness and `tokio-postgres`; external Prisma, psql, and SQLAlchemy probes are opt-in when separately provisioned. Version-pinned pgAdmin 9.16 and DBeaver 26.1.3 normalized trace-replay lanes are available and retain explicitly non-certifying manifests. The psycopg 3 and JDBC client lanes remain Planned/unverified, and pgAdmin/DBeaver remain uncertified until separately provisioned live desktop workflows retain exact-revision evidence. Client-version upgrades require refreshed traces and live smoke suites before a future certification claim changes.

The shared dispatch contract for these lanes is documented in [Compatibility Probe Contract](compatibility-probe-contract.md)
with the archived dispatch interface in the [Compatibility Probes workflow at the frozen baseline](https://github.com/cntryl/cassie/blob/5f9bfd807d4e647401fbb8a587429c48425af61e/.github/workflows/compatibility-probes.yml). Current local coverage runs through `cargo test --locked --test compatibility`; external dispatch requires a separately provisioned runner.

## Intentional Differences

- No full PostgreSQL parity or extension ABI.
- PostgreSQL server filesystem helpers may have built-in `pg_proc` identities for catalog lookup, but invoking them returns SQLSTATE `0A000`; Cassie does not expose server file or directory access.
- Midge-backed values have no PostgreSQL TOAST chunks; `pg_column_toast_chunk_id(any)` retains its OID result type and returns SQL NULL.
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
The [wire transaction contract](pgwire-transaction-contract.md) records its
implemented state tables and focused independent-session, unnamed-object and
extended-to-simple acceptance cases. Eligible Simple read/DML batches commit at
successful Query completion; eligible extended work remains private until Sync.
A healthy extended-to-Simple handoff joins atomically, and implicit failure rolls
back the owning segment. Explicit blocks retain T/E across Sync until completion
or recovery. DDL stays standalone, explicit table COPY staging remains supported,
and extended COPY stays unsupported. Broader portal, publication and packet/client
qualification remains with #757/#763/#780; these finite cases add no transactional
DDL, stronger isolation or PostgreSQL server-parity promise.

Scalar expression subqueries are not implemented; implemented table/predicate/
lateral/correlated forms remain evaluation surfaces. Full PostgreSQL server,
internal catalog, extension and pgvector ABI parity remain unclaimed. External
driver, trace replay and actual live desktop evidence remain separate; new client
versions need new finite-workflow evidence.
