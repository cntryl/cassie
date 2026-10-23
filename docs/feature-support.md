# Feature Support

This is the sole canonical behavior and status matrix for Cassie. Each capability has exactly one status:

- **Stable**: implemented, tested, documented, and supported within the current pre-release baseline.
- **Experimental**: implemented for documented cases, but correctness, resource bounds, evidence, or compatibility still has an open closure item.
- **Planned**: accepted future work that is not implemented as a supported contract.

`Production-ready` is not a feature status. It is an evidence classification owned by [Production Readiness](production-readiness.md).

The current beta support envelope consists of capabilities marked Stable. Experimental capabilities ship for evaluation with explicit limits and are not compatibility commitments. See [Production Readiness](production-readiness.md) for the release evidence bar.

The [Local Development Readiness](local-development-readiness.md) gate narrows
the end-to-end workflows required for repeatable local use. It does not promote
any capability's support status before its acceptance evidence passes.

## Ownership Boundary

Midge owns persistence, durability, and recovery mechanics. Cassie owns logical query layouts and query-visible failures, including SQL semantics, indexes, planning, execution, caching, cancellation, memory and result limits, and protocol error mapping. Cassie does not implement a parallel WAL, recovery engine, or storage abstraction.

Cassie is permanently a single-node query engine. Distributed SQL, cluster membership or management, replication, consensus, sharding and rebalancing, cross-node transactions, multi-node planning, remote query forwarding, and automatic cross-node repair are product non-goals. External systems may route to independent nodes, but that does not expand Cassie's execution or coordination boundary.

The only accepted Cassie-owned on-disk baseline marker is `cassie-midge-layout-v3`. A directory without that marker is rejected with a recreate diagnostic. This layout has not shipped, so no migration utility is required; pre-release v2 directories must be recreated before use.

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
| Aggregate-local modifiers | `COUNT`, `SUM`, `AVG`, `MIN`, `MAX`, and text `STRING_AGG` accept selected `FILTER`, `DISTINCT`, and aggregate-local `ORDER BY` forms for supported input types. `FILTER` runs before argument evaluation. `DISTINCT` and ordered arguments use the controlled serial row accumulator; typed and column-batch summary paths decline modified calls, and parallel partial aggregation declines `DISTINCT` and ordered arguments. The fallback retains query-memory admission and cancellation. Focused evidence: [core aggregate modifiers](../tests/sql_queries/aggregates/semantics/semantics.rs), [typed fallback](../src/executor/execution/aggregate_exec/typed/tests.rs), [column-summary fallback](../tests/analytics/aggregates/aggregate_acceleration.rs), and [serial DISTINCT fallback and lifecycle](../src/executor/execution/aggregate_exec/modifier_tests.rs). | Experimental |
| Text `STRING_AGG` | TEXT/CHAR/VARCHAR values and delimiters; skips NULL values, treats a NULL delimiter as empty, and returns SQL NULL for empty or all-NULL input. Its `DISTINCT` identity includes both arguments and its modifiers follow the aggregate-local contract above; result type is TEXT. BYTEA `STRING_AGG` and ARRAY/JSON collection aggregates remain pending under [#811](https://github.com/cntryl/cassie/issues/811), whose array and JSON prerequisites are still open. | Experimental |
| Selected scalar text functions | `split_part` (including negative field positions), `btrim`/`ltrim`/`rtrim`, `substr`, `strpos`/`position`, `replace` (all occurrences), selected `SUBSTRING` and `TRIM` forms, `format` with `%s`, `%I`, `%L`, `%%`, positional arguments, static/dynamic widths and `-`, plus `overlay`, `lpad`/`rpad`, `repeat`, `reverse`, `initcap`, `translate` and `concat_ws`. Text positions, padding, and reversal count Unicode scalar values. These functions preserve the selected SQL NULL rules; `concat_ws` skips NULL value arguments and returns NULL for a NULL separator. The selected `initcap` witness is ASCII word capitalization; runtime case mappings use Rust's Unicode tables, without locale-specific collation tailoring. Generated output is query-memory-reserved before allocation; long loops check cancellation and deadlines. Normalization/casefold/Unicode-assignment helpers, conversion-to-ASCII, quoting/radix helpers, and table-valued string functions are not selected. Focused evidence: [`string_functions.rs`](../tests/sql_queries/functions/string/string_functions.rs), [`format`](../tests/pgwire_extended/functions/string/format), and [#803](https://github.com/cntryl/cassie/issues/803)/[#1034](https://github.com/cntryl/cassie/issues/1034). | Experimental |
| Grouping sets | Single-expression `GROUPING SETS (expression)`, equivalent to `GROUP BY expression`; multiple or empty sets, `ROLLUP`, `CUBE`, `GROUPING()`, and `GROUP BY DISTINCT` remain unsupported. | Experimental |
| Join syntax | Inner, left, right, full outer, cross, lateral, apply, semi, and anti forms with legality-preserving planning | Experimental |
| Subqueries | Implemented table, predicate (including EXISTS), lateral, and correlated forms. | Experimental |
| Qualified relation wildcards | `relation.*` expands only the matching source namespace across base tables, CTEs, and derived relations; unknown qualifiers fail binding. Existing wildcard identity-hiding rules apply. | Experimental |
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

SQL type positions reject bare `DOUBLE` and `STRING`, which PostgreSQL does not
expose as type names. Use `DOUBLE PRECISION`, `FLOAT` or `FLOAT8` for the
existing OID 701 identity and `TEXT` for OID 25. This also rejects arrays of
the bare aliases. The public `DataType::parse_sql` helper retains the legacy
`double` and `string` aliases for non-SQL callers and does not define SQL
syntax acceptance.

`FLOAT(p)` casts and declared columns use the existing REAL identity (OID 700)
for precision 1 through 24 and FLOAT identity (OID 701) for precision 25
through 53. The selected carrier controls value rounding; the precision itself
is not retained as a typmod. Precision 0 and 54 are rejected as SQLSTATE 22023;
the signed spelling `FLOAT(+24)` is rejected as SQLSTATE 42601.
The PostgreSQL 18.6 oracle returns `1.2345679` for the selected REAL input and
`1.23456789` for FLOAT. The parser regression
[`should_map_float_precision_boundaries_to_postgres_type_oids`](../tests/parser_types/grammar/metadata.rs)
checks both precision boundaries, endpoint values, rounding, and invalid
precisions; its companion verifies declared-column values and metadata. The
extended-protocol regression
[`should_bind_float_precision_casts_using_native_type_metadata`](../tests/pgwire_extended/types/real/float_precision.rs)
checks inferred parameter OIDs, statement and portal RowDescriptions, values,
NULLs, and SQLSTATE 22023 at both invalid bounds. This qualifies only canonical
`FLOAT(p)` handling; broader type, cast, and array behavior remains open under
[#796](https://github.com/cntryl/cassie/issues/796).

Extended-protocol casts also accept the schema-qualified
`pg_catalog.timestamptz(3)`, `pg_catalog.timetz(3)` and
`pg_catalog.numeric(5,2)` spellings. The pgwire regression
[`should_bind_pg_catalog_qualified_temporal_casts_with_native_metadata`](../tests/pgwire_extended/types/temporal/parameters.rs)
checks inferred parameter OIDs, statement and portal descriptors, typmods, and
the rounded/rendered values. Its PostgreSQL 18.6 reference returns
`2001-02-17 01:38:40+00`, `20:38:40-05` and `12.35` for the selected inputs.
This qualifies only these type spellings; the remaining #796 cast, type-name,
literal, operator, collation and expression obligations stay open.

The selected `bpchar` spelling is accepted as the existing character type:
`bpchar` and `pg_catalog.bpchar` are unbounded, while `bpchar(n)` and
`pg_catalog.bpchar(n)` retain the declared character width. The pgwire witness
[`should_bind_bpchar_casts_using_native_type_metadata`](../tests/pgwire_extended/types/character/bpchar_casts.rs)
checks inferred parameter OIDs, statement descriptors, values and padding. The
separate `name` type and broader collation behavior remain pending under #796.
Until that type is selected, both `name` and `pg_catalog.name` casts are
rejected, as covered by
[`should_reject_unselected_name_type_casts`](../tests/parser_types/grammar/expressions/semantics.rs).

The #796 extended-protocol witness
[`should_bind_the_selected_four_type_cast_fixture_through_extended_protocol`](../tests/pgwire_extended/grammar/parameters.rs)
executes inferred `uuid`, `jsonb`, `numeric(18,2)`, and `uuid[]` casts through
Parse, Describe, Bind, and Execute, including rounded numeric input and
all-NULL parameters. It records JSONB under the JSON OID and UUID arrays under
Cassie's private array OID and JSON-array text representation. Matching array
source and target types preserve the existing array value carrier; this does
not qualify PostgreSQL array ABI or other array casts.

For selected scalar types, a postfix `::` cast can follow a parenthesized
`CAST` result. The parser regression
[`should_bind_parenthesized_cast_result_through_postfix_conversion`](../tests/parser_types/grammar/parameters.rs)
checks `(CAST('42' AS TEXT))::INT` against the equivalent nested `CAST`,
including the INT result OID and value. This qualifies that chain shape only;
other cast pairs and syntax remain pending under #796.

A chained postfix cast also accepts a schema-qualified type modifier:
`'1.234'::pg_catalog.numeric(5,2)::pg_catalog.text`. The parser regression
[`should_bind_chained_qualified_type_modifier_casts`](../tests/parser_types/grammar/parameters.rs)
compares it with nested `CAST`, verifies the rounded `1.23` text value, and
checks the TEXT result OID. This witness covers the combined spelling and cast
shape.

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

The selected `UPDATE ... RETURNING` arithmetic witness in
[`should_return_parenthesized_row_arithmetic_from_update`](../tests/pgwire_extended/grammar/returning_arithmetic.rs)
checks a row-dependent `CASE` branch containing `id / (id - id)`, plus the
zero-row form. Both succeed on the current source; returned values stay `1` and
`2`, the empty result retains its BIGINT OID 20 descriptor, and a readback
confirms both statements leave stored values unchanged. These exact forms were
historical failures recorded in
[#796](https://github.com/cntryl/cassie/issues/796); other forms in that issue
remain open.

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

`_id` can be named directly against a base table in a select list, `WHERE`, and `ORDER BY`, and always returns the internal identity, including on a table that declares its own `id`. `ORDER BY <indexed column>, _id` keeps the ordered scalar-index read path with the identity as a deterministic tiebreaker. A derived relation exposes only the columns it projects, so `id` or `_id` that its body does not project is an unresolvable column reference rather than NULL. In a select list, `relation.*` expands only columns visible through that source namespace, including for CTEs and derived relations; an unknown qualifier fails binding.

Adding an `id` column with `ALTER TABLE ... ADD COLUMN id ...` switches a bare `id` on that table from the internal identity to the new column. Without a default, rows written before the change read as NULL; with a literal default, those rows receive the coerced default value. `_id` is unaffected.

REST document endpoints report the internal identity as `"id"` in their JSON body. That field is the SQL `_id` of the row, not a declared `id` column.

## Mutation and Catalog

| Capability | Behavior | Status |
| --- | --- | --- |
| Unique constraint index forms | Table-level UNIQUE constraints in `CREATE TABLE` and `ALTER TABLE` accept `INCLUDE` payload columns and scalar btree `WITH (fillfactor=..., deduplicate_items=...)`. Duplicate, key-overlapping, and unknown INCLUDE fields are rejected. `ALTER TABLE ... ADD [CONSTRAINT name] UNIQUE USING INDEX name` adopts a unique btree index with default key ordering and no expression keys or partial predicate; a named constraint renames the index, preserves its catalog identity, NULL policy, and uniqueness reservations, and dropping the constraint removes it. Constraint deferrability (`DEFERRABLE`, `NOT DEFERRABLE`, `ALTER CONSTRAINT`, and `SET CONSTRAINTS`) is unsupported under the selected read-model profile and returns SQLSTATE `0A000`; this is an absent capability, not a regression. Evidence: [`should_preserve_include_columns_on_a_unique_constraint_index`](../tests/compatibility/lifecycle/constraints/extended_constraint_forms.rs), [`should_normalize_include_field_spelling_for_altered_unique_constraints`](../tests/compatibility/lifecycle/constraints/extended_constraint_forms.rs), [`should_attach_an_existing_unique_index_as_a_table_constraint`](../tests/compatibility/lifecycle/constraints/extended_constraint_forms.rs), [`should_keep_composite_unique_enforcement_after_constraint_adoption`](../tests/compatibility/lifecycle/constraints/extended_constraint_forms.rs), [`should_preserve_nulls_not_distinct_when_adopting_unique_index`](../tests/compatibility/lifecycle/constraints/extended_constraint_forms.rs), and [`should_protect_constraint_owned_index_until_constraint_drop`](../tests/compatibility/lifecycle/constraints/constraint/semantics.rs). | Experimental |
| DML | `INSERT`, PostgreSQL-style `ON CONFLICT`, `UPDATE`, `DELETE`, `RETURNING`, CSV copy ingestion | Experimental |
| Transactions | Begin, commit, rollback, savepoints, read-your-writes and the bounded implicit Query/Sync profile with atomic healthy handoff; selected database-scoped schema, table, scalar-index, and materialized-view DDL can run in explicit transactions and publish atomically with staged rows. Plain READ COMMITTED SQL INSERT waits for a competing transaction that owns the same non-null single-column PRIMARY KEY or UNIQUE constraint value; broader unique-key wait paths remain pending under #819. DDL inside implicit transactions and unselected DDL inside explicit transactions remain rejected. Standalone DDL and explicit table COPY boundaries follow the [wire transaction contract](pgwire-transaction-contract.md); broader transactional DDL acceptance remains under #833. | Experimental |
| Database and schema scope | Databases, schemas, persisted `search_path`, qualified names, and administrator-managed `CONNECT` grants with live-session revalidation | Stable |
| Tables and constraints | Table DDL, name-idempotent `CREATE TABLE IF NOT EXISTS`, and `DROP TABLE IF EXISTS` as a no-op for a missing relation; defaults, unique, check, and foreign key. `ALTER TABLE ... ADD COLUMN IF NOT EXISTS` is a name-only no-op for an existing field and applies a literal default to historical rows, including rows staged earlier in an explicit transaction; reads in that transaction see the new field and its backfilled values before commit. Rollback, restart recovery, partial backfill replay, and backward decoding of pending records are covered by [`should_add_column_if_not_exists_without_replacing_existing_default`](../tests/compatibility/lifecycle/ddl/ddl_extensions/semantics.rs), [`should_rollback_alter_table_column_addition`](../tests/compatibility/lifecycle/ddl/ddl_extensions/transactions.rs), [`should_apply_added_literal_default_to_rows_staged_before_alter_commit`](../tests/compatibility/lifecycle/ddl/ddl_extensions/transactions.rs), [`should_preserve_existing_column_default_when_add_if_not_exists_runs_in_transaction`](../tests/compatibility/lifecycle/ddl/ddl_extensions/transactions.rs), [`should_commit_added_literal_default_backfill_without_other_staged_writes`](../tests/compatibility/lifecycle/ddl/ddl_extensions/transactions.rs), [`should_replay_add_column_derived_state_after_schema_commit_interruption`](../tests/catalog_security/ddl/schema_operation_recovery.rs), [`should_resume_add_column_default_backfill_after_batch_interruption`](../tests/catalog_security/ddl/schema_operation_recovery.rs), and [`should_decode_pending_field_add_without_default_from_legacy_record`](../src/midge/adapter/schema_ops_tests.rs). SERIAL sequence catalog identity and ownership follow table rename through local restart; see [`should_rename_serial_sequence_when_renaming_its_table`](../tests/sql_mutations/ddl/migration_ddl_sequences.rs). Multi-action `ALTER TABLE`, `ALTER COLUMN TYPE USING`, `ALTER TABLE SET SCHEMA`, and `DROP SCHEMA CASCADE` remain pending under [#839](https://github.com/cntryl/cassie/issues/839). | Experimental |
| Table access methods | `CREATE TABLE ... USING heap` and `ALTER TABLE ... SET ACCESS METHOD heap` select the existing Midge row-store path; `pg_class.relam` reports OID 2 and `pg_am` identifies it as `heap`. Other table access methods, materialized-view access-method forms, and column `STORAGE`/`COMPRESSION` choices remain pending under [#826](https://github.com/cntryl/cassie/issues/826) and are rejected with SQLSTATE `0A000`. Catalog identity for Cassie-native vector, column, and time-series indexes remains pending under [#817](https://github.com/cntryl/cassie/issues/817); their `pg_class.relam` remains 0 until their access methods receive selected catalog identities. Evidence: [`should_create_table_with_an_explicit_access_method`](../tests/compatibility/lifecycle/tables/table_storage_access.rs), [`should_change_table_access_method_without_changing_rows`](../tests/compatibility/lifecycle/tables/table_storage_access.rs), [`should_reject_column_storage_options_without_publishing_the_table`](../tests/compatibility/lifecycle/tables/table_storage_access.rs), [`should_reject_column_compression_options_without_publishing_the_table`](../tests/compatibility/lifecycle/tables/table_storage_access.rs), [`should_preserve_column_storage_compression_metadata`](../tests/compatibility/lifecycle/tables/table_storage_access.rs) (pending, ignored), and [`should_leave_unselected_native_index_methods_without_catalog_oids`](../src/catalog/virtual_views_pg.rs). | Experimental |
| `CREATE TABLE AS` | Supports `CREATE TABLE [IF NOT EXISTS] name [(column names)] [USING heap] AS` with `SELECT`, `TABLE`, `VALUES`, or `EXECUTE` of a prepared query, plus `WITH DATA` and `WITH NO DATA` (default: `WITH DATA`). `USING heap` selects the default row-store path; other access methods are rejected. Output types and modifiers come from query metadata; target columns are nullable unless a separate declaration adds constraints. Creation and materialization publish atomically, and a failed query leaves no table. `IF NOT EXISTS` skips query planning when the relation already exists. Cassie's implicit row identity remains visible as `id` in `SELECT *` when the CTAS target does not declare an `id`; explicitly project the named CTAS columns when reading only the copied output. Evidence: [`should_create_table_as_an_existing_table_with_column_names`](../tests/compatibility/lifecycle/tables/table_forms/create_table_as.rs), [`should_roll_back_ctas_table_when_query_materialization_fails`](../tests/compatibility/lifecycle/tables/table_forms/create_table_as.rs), [`should_skip_existing_relation_for_create_table_as_if_not_exists`](../tests/compatibility/lifecycle/tables/table_forms/semantics.rs), [`should_create_table_as_values_with_result_metadata`](../tests/compatibility/lifecycle/tables/table_forms/metadata.rs), [`should_create_table_as_prepared_execute_with_typed_parameters`](../tests/compatibility/lifecycle/tables/table_forms/parameters.rs), [`should_preserve_ctas_type_modifiers_without_materializing_rows`](../tests/compatibility/lifecycle/tables/table_forms/semantics.rs), [`should_create_ctas_with_an_explicit_access_method`](../tests/compatibility/lifecycle/tables/table_storage_access.rs), and [`should_materialize_explicit_with_data_ctas_rows`](../tests/compatibility/lifecycle/tables/lifecycle/semantics.rs). Explicit column storage options and the other table-cloning requirements remain pending under [#826](https://github.com/cntryl/cassie/issues/826). | Experimental |
| Unique constraint NULL policy | Column- and table-level `UNIQUE NULLS [NOT] DISTINCT`, including composite keys, follows PostgreSQL duplicate-NULL behavior. Each unique constraint has a scalar backing index exposed through `pg_constraint.conindid`; dropping the index directly reports `2BP01`, and dropping the constraint removes its index. Evidence: [`should_reject_duplicate_null_keys_for_column_nulls_not_distinct`](../tests/compatibility/lifecycle/constraints/constraint/errors.rs), [`should_enforce_table_unique_nulls_not_distinct_constraint`](../tests/compatibility/lifecycle/constraints/constraint_control.rs), [`should_enforce_composite_unique_nulls_not_distinct_constraint`](../tests/compatibility/lifecycle/constraints/constraint_control.rs), and [`should_drop_unique_constraint_backing_index_with_constraint`](../tests/compatibility/lifecycle/constraints/constraint_control.rs). The catalog retains Cassie's documented text display-label representation for index identities. | Experimental |
| `CREATE TABLE ... LIKE` | Copies source columns, types, and `NOT NULL` properties. `INCLUDING DEFAULTS` copies non-sequence defaults; repeated `DEFAULTS` actions use the last action. Sequence-backed defaults and other LIKE options, including constraints, indexes, identity, and `ALL`, return SQLSTATE `0A000` until their dependency behavior is supported. Remaining PostgreSQL LIKE behavior is tracked by [#826](https://github.com/cntryl/cassie/issues/826). | Experimental |
| Materialized SQL views | `CREATE MATERIALIZED VIEW [IF NOT EXISTS] ... AS SELECT` supports output-column names and `WITH DATA`/`WITH NO DATA`; initial data is built before catalog publication, so a failed build leaves no view. Scanning before population returns SQLSTATE `55000`. `REFRESH` supports `WITH DATA`/`WITH NO DATA`; `REFRESH ... CONCURRENTLY` builds a replacement output version and swaps it only after its indexes are ready. Concurrent refresh requires a populated view and a full-row unique scalar index over named columns; partial or expression indexes do not qualify, and `WITH NO DATA` is rejected. Failed replacement index builds leave the active output populated and unchanged. `DROP` removes the view; `pg_matviews.ispopulated` follows the active output version. Selected explicit transactions provide same-session visibility, commit/rollback behavior, and observer isolation; populated rows survive local restart. Focused evidence: [`should_remove_failed_materialized_view_creation`](../tests/sql_mutations/transactions/transactional_ddl.rs), [`should_reject_view_commands_for_materialized_projection`](../tests/sql_mutations/transactions/transactional_ddl.rs), [`should_report_unpopulated_view_scan_inside_transaction`](../tests/sql_mutations/transactions/transactional_ddl.rs), [`should_publish_materialized_view_after_commit`](../tests/sql_mutations/transactions/transactional_ddl.rs), [`should_rollback_materialized_view_publication_with_transaction`](../tests/sql_mutations/transactions/transactional_ddl.rs), [`should_rollback_materialized_view_refresh`](../tests/sql_mutations/transactions/transactional_ddl.rs), [`should_recover_persisted_materialized_view_after_restart`](../tests/sql_mutations/transactions/restart_publication.rs), [`should_require_a_unique_index_for_concurrent_materialized_view_refresh`](../tests/compatibility/lifecycle/tables/table_forms/semantics.rs), [`should_preserve_active_materialized_view_when_concurrent_refresh_cannot_rebuild_unique_index`](../tests/compatibility/lifecycle/tables/table_forms/semantics.rs), [`should_reject_concurrent_refresh_of_unpopulated_materialized_view`](../tests/compatibility/lifecycle/tables/table_forms/semantics.rs), and [`should_reject_concurrent_refresh_with_no_data`](../tests/compatibility/lifecycle/tables/table_forms/semantics.rs). Incremental refresh, rename, access methods, catalog relation identity, ordinary-refresh index retention, and dependent COPY/comment/extension metadata remain pending under [#826](https://github.com/cntryl/cassie/issues/826), [#839](https://github.com/cntryl/cassie/issues/839), [#842](https://github.com/cntryl/cassie/issues/842), [#843](https://github.com/cntryl/cassie/issues/843), [#845](https://github.com/cntryl/cassie/issues/845), and [#846](https://github.com/cntryl/cassie/issues/846). Current rejection-boundary checks cover materialized-view rename (`0A000`), explicit access-method creation and changes (`0A000`), and comments (`0A000`) in [`semantics.rs`](../tests/compatibility/lifecycle/tables/table_forms/semantics.rs), [`table_storage_access.rs`](../tests/compatibility/lifecycle/tables/table_storage_access.rs), and [`comment_catalog_coverage.rs`](../tests/compatibility/lifecycle/comments/comment_catalog_coverage.rs); these checks do not qualify positive support. The broader table-creation issue remains open as [#826](https://github.com/cntryl/cassie/issues/826). | Experimental |
| Materialized-view scalar indexes | A populated SQL materialized view accepts scalar btree indexes, including `UNIQUE`; index creation validates existing output rows and rejects duplicate keys with SQLSTATE `23505`. `pg_indexes` and `pg_index.indrelid` expose the logical materialized-view name and uniqueness, `pg_matviews.hasindexes` reflects its indexes, and `DROP INDEX` removes the catalog state. Ordinary and concurrent refresh rebuild these indexes on a new output version before activation; a failed unique-index rebuild leaves the prior output active. Evidence: [`should_create_unique_index_on_populated_sql_materialized_view`](../tests/compatibility/lifecycle/indexes/materialized_view_indexes.rs), [`should_remove_unique_index_from_sql_materialized_view_catalog`](../tests/compatibility/lifecycle/indexes/materialized_view_indexes.rs), [`should_reject_unique_index_when_materialized_view_has_duplicate_keys`](../tests/compatibility/lifecycle/indexes/materialized_view_indexes.rs), [`should_preserve_active_materialized_view_when_concurrent_refresh_cannot_rebuild_unique_index`](../tests/compatibility/lifecycle/tables/table_forms/semantics.rs), [`should_require_a_unique_index_for_concurrent_materialized_view_refresh`](../tests/compatibility/lifecycle/tables/table_forms/semantics.rs), and [`should_serialize_materialized_view_refresh_modes`](../tests/analytics/projections/projection_concurrency.rs). SQL materialized-view `pg_class` identity remains pending under [#826](https://github.com/cntryl/cassie/issues/826). | Experimental |
| Scalar indexes | Primary, secondary, composite, unique (including `NULLS [NOT] DISTINCT` for nullable unique keys), covering, partial, expression, name-idempotent `CREATE INDEX IF NOT EXISTS`, and generated names for `CREATE INDEX ON table (...)`; `DROP INDEX [schema.]name` resolves the index through the current database and search path, while an explicit schema is exact. The legacy `DROP INDEX name ON table` form remains accepted. Nonconcurrent `REINDEX INDEX name` and `REINDEX TABLE name` rebuild selected scalar indexes from stored rows outside active transactions; REINDEX is not part of the selected transactional DDL set. `(VERBOSE FALSE, CONCURRENTLY FALSE)` is accepted as an explicit default option set. Schema, database, and system targets, concurrent rebuilds, verbose output, and tables containing non-scalar indexes remain unsupported. `pg_indexes.indexdef` reports the schema-qualified table and access method; `pg_get_indexdef` renders stored scalar expression keys as SQL, including casts, CASE, and unary expressions. `NULLS NOT DISTINCT` is persisted, rejects duplicate NULL keys, and is exposed through `pg_index.indnullsnotdistinct` and `pg_get_indexdef`; ordinary unique indexes keep PostgreSQL’s default NULL-distinct behavior. Scalar btree keys accept `ASC`/`DESC` and `NULLS FIRST`/`LAST`; `text_pattern_ops` is accepted for text-like fields. These key modifiers and the selected operator class are persisted and exposed through `pg_index` and `pg_get_indexdef` as catalog metadata only; they do not change Midge key encoding, comparison behavior, or query acceleration. Other operator classes and scalar `USING hash` are rejected until their semantics are implemented. Selected evidence is in [`should_render_supported_expression_index_definitions`](../tests/compatibility/lifecycle/indexes/expression_index_definition.rs), [`should_publish_unique_index_nulls_not_distinct_metadata`](../tests/compatibility/lifecycle/indexes/nulls_not_distinct.rs), [`should_publish_partial_btree_index_clause_metadata`](../tests/compatibility/lifecycle/indexes/included_partial_indexes.rs), [`should_preserve_operator_class_with_mixed_btree_key_modifiers`](../tests/compatibility/lifecycle/indexes/operator_class_key_modifiers.rs), [`should_reject_unselected_btree_operator_classes`](../tests/compatibility/lifecycle/indexes/operator_class_key_modifiers.rs), [`should_persist_include_index_metadata_after_restart`](../tests/storage_indexes/indexes/scalar_indexes/restart.rs), [`should_generate_postgresql_name_for_an_unnamed_index`](../tests/compatibility/lifecycle/indexes/generated_index_name.rs), [`should_report_created_index_definition_in_pg_indexes`](../tests/catalog_security/catalogs/indexes/catalog_information/semantics.rs), [`should_reject_hash_indexes_until_storage_supports_the_method`](../tests/compatibility/lifecycle/indexes/index.rs), [`should_rebuild_an_existing_index_with_reindex`](../tests/compatibility/lifecycle/indexes/index.rs), [`should_rebuild_all_indexes_for_a_table`](../tests/compatibility/lifecycle/indexes/index.rs), [`should_reindex_with_explicit_nonconcurrent_options_while_preserving_rows`](../tests/compatibility/lifecycle/indexes/reindex_options.rs), [`should_preserve_partial_index_results_after_reindex_with_null_keys`](../tests/compatibility/lifecycle/indexes/reindex_options.rs), [`should_reject_reindex_inside_an_explicit_transaction`](../tests/compatibility/lifecycle/indexes/reindex_options.rs), [`should_serialize_scalar_index_rebuild_with_collection_writes`](../src/midge/adapter/scalar_indexes.rs), [`should_not_rebuild_scalar_index_from_metadata_removed_before_gate_acquisition`](../src/midge/adapter/scalar_indexes.rs), [`should_drop_an_index_without_an_on_clause`](../tests/compatibility/lifecycle/ddl/ddl_extensions/semantics.rs), [`should_drop_only_the_schema_qualified_index_without_an_on_clause`](../tests/compatibility/lifecycle/indexes/index.rs), [`should_drop_the_first_unqualified_index_in_search_path_order`](../tests/compatibility/lifecycle/indexes/drop_index_if_exists_scope.rs), and [`should_find_exact_numeric_index_keys_that_differ_beyond_float8_precision`](../tests/sql_queries/types/numeric/numeric.rs). `DROP INDEX CONCURRENTLY` is explicitly rejected as unsupported syntax and remains outside the selected scalar-index contract under #842; [`should_reject_drop_index_concurrently_as_unsupported_syntax`](../tests/parser_types/grammar/indexes/semantics.rs) records the boundary. Rename, remaining index-shape cases, complete read/write maintenance, and broader restart qualification remain pending. `COMMENT ON INDEX` is unsupported and remains pending under [#845](https://github.com/cntryl/cassie/issues/845). | Experimental |
| Physical ColumnStore tables | Field-key table layout in the owning Midge database family; separate from Stable CBM2 sidecars and the Planned common typed-column pipeline. Layout, lifecycle and typed-reader qualification remain under [#768](https://github.com/cntryl/cassie/issues/768). | Experimental |
| `information_schema.tables` | Stable named-client table/view discovery columns documented in the catalog support contract | Stable |
| `information_schema.columns` | Stable named-client column discovery columns documented in the catalog support contract; integer and floating-point types use the PostgreSQL names `integer` and `double precision` | Stable |
| Remaining virtual catalogs | PostgreSQL-like and Cassie runtime views outside the stable named-client subset; no PostgreSQL-internal parity claim | Experimental |
| Projection lifecycle | Materialized projection create, refresh, checkpointed replay, version build, verification, activation, rollback by reactivation, repair, and drop on one local instance, as defined in [Materialized Projection Lifecycle](materialized-projection-lifecycle.md). Interrupted or failed builds never become query-visible; stale or unproven projections fall back to authoritative source rows. Column-store table mode, remote replay coordination, and automatic activation are outside this surface. | Stable |
| Verification and repair | Hashes, manifests, local repair planning and audit for materialized projections; see [Materialized Projection Lifecycle](materialized-projection-lifecycle.md). No general (non-projection) index verification or repair exists. | Experimental |

### Pending materialized-view compatibility behavior

The following positive scenarios remain pending; the rejection checks above do
not qualify them as supported:

The original positive assertions remain in their owner test modules as
explicit `#[ignore]` cases. They are pending, not passed, and do not
count as current support evidence.

- Under [#839](https://github.com/cntryl/cassie/issues/839), rename
  `compat_mv_rename` to `compat_mv_renamed`, keep its output readable, and
  report the new name in `pg_matviews`.
- Under [#826](https://github.com/cntryl/cassie/issues/826), qualify
  `CREATE MATERIALIZED VIEW ... USING heap` and
  `ALTER MATERIALIZED VIEW ... SET ACCESS METHOD heap` only after the storage
  contract is defined; accepted changes must preserve output rows.
- Materialized-view comments remain dependent metadata under
  [#845](https://github.com/cntryl/cassie/issues/845): setting a comment with
  `COMMENT ON MATERIALIZED VIEW ... IS 'cached rows'` and clearing it with
  `IS NULL` must follow the selected relation-comment and catalog contract.
- Extension dependencies remain under [#843](https://github.com/cntryl/cassie/issues/843)
  and [#826](https://github.com/cntryl/cassie/issues/826): `ALTER INDEX ...
  DEPENDS ON EXTENSION pgcrypto` and `ALTER MATERIALIZED VIEW ... DEPENDS ON
  EXTENSION pgcrypto` must publish and remove the corresponding dependency
  metadata transactionally. #843 remains v0.4 and #845 still depends on #817.

### Scalar btree storage parameters

Scalar btree indexes accept `fillfactor` values from 10 through 100 and
`deduplicate_items` values `on` or `off` in `CREATE INDEX ... WITH (...)` and
`ALTER INDEX ... SET/RESET (...)`. Cassie stores these values with the index
metadata and reports them through `pg_class.reloptions` and
`pg_get_indexdef`; they do not change Midge index layout or query behavior.
Other btree parameters and GIN, GiST, and BRIN storage parameters remain
unsupported. Focused evidence is in
[`should_update_index_storage_parameter_catalog_state`](../tests/compatibility/lifecycle/indexes/storage_parameter_options.rs),
[`should_store_btree_deduplicate_items_option`](../tests/compatibility/lifecycle/indexes/storage_maintenance/semantics.rs),
and [`should_ignore_alter_index_if_exists_for_a_missing_index`](../tests/compatibility/lifecycle/indexes/storage_maintenance/semantics.rs).
SQL GIN, GiST, BRIN, and SP-GiST access methods and their method-specific
operator classes are unsupported; Cassie's full-text retrieval uses a separate
internal index path. GIN pending-list cleanup and BRIN range-maintenance
functions are not implemented. PostgreSQL `VACUUM`, `ANALYZE`, `CLUSTER`, and
`CHECKPOINT` statements are unsupported and return SQLSTATE `0A000`; they do not
change table rows. Rejection evidence is in
[`selected_access_methods.rs`](../tests/compatibility/lifecycle/indexes/selected_access_methods.rs),
[`should_reject_gin_storage_parameters_until_the_method_is_supported`](../tests/compatibility/lifecycle/indexes/storage_maintenance/parameters.rs),
[`should_reject_brin_pages_per_range_until_the_method_is_supported`](../tests/compatibility/lifecycle/indexes/storage_maintenance/semantics.rs),
and the storage-maintenance rejection tests in that file. `ALTER INDEX ...
ALTER COLUMN ... SET STATISTICS` is also unsupported: index-attribute rows and
statistics targets remain pending under [#817](https://github.com/cntryl/cassie/issues/817),
which still depends on the selected #777 catalog/client workflow.

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
| Built-in extension compatibility | Planned finite `vector` and `pgcrypto` compatibility shims: extension catalog/lifecycle behavior and `pgcrypto.digest(text|bytea, 'sha256')`; vector SQL, type and retrieval semantics are owned by #844. Cassie does not load native extension libraries or claim arbitrary extension ABI support. `CREATE EXTENSION pgcrypto` currently returns SQLSTATE `0A000`; [`should_reject_pgcrypto_installation_until_extension_compatibility_is_implemented`](../tests/compatibility/lifecycle/extensions/dependency_targets.rs) records the v0.4 boundary under #843. | Planned |
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
