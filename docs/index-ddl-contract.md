# Index DDL contract for row-authoritative tables

This note records the bounded SQL index contract owned by [issue #842](https://github.com/cntryl/cassie/issues/842) for the v0.2.0 Query and Retrieval milestone. It extends the current [SQL Compatibility Profile](sql-dialect-profile.md) and [Feature Support](feature-support.md); it does not establish PostgreSQL-wide index parity.

PostgreSQL 18.6 is the semantic reference for selected syntax and catalog
behavior: [CREATE INDEX](https://www.postgresql.org/docs/18/sql-createindex.html),
[index types](https://www.postgresql.org/docs/18/indexes-types.html),
[DROP INDEX](https://www.postgresql.org/docs/18/sql-dropindex.html), and
[REINDEX](https://www.postgresql.org/docs/18/sql-reindex.html). PostgreSQL's
documents describe the reference behavior; Cassie's local tests are the
executable evidence for the Cassie contract.

## Storage boundary and method names

Midge 0.5.2 publicly exposes an engine, key-value transactions, and ordered
key/value scans. It does not expose SQL index access methods or PostgreSQL
operator classes. Cassie owns its SQL index definitions and builds index state
inside the existing Midge database family. No second storage layer or Midge
engine change is part of this contract.

| SQL method | Cassie behavior | Boundary |
| --- | --- | --- |
| Omitted or `USING btree` | Row-authoritative scalar index. This is the selected method for primary, secondary, composite, unique, expression, partial, and covering indexes. | The scalar entries use Cassie key encoding in Midge. PostgreSQL key modifiers and selected operator-class names are catalog metadata; they do not change Midge key encoding, comparison, or acceleration. |
| `USING fulltext` | Cassie full-text posting index for one `TEXT` field, used by Cassie's `search`, `search_score`, and `snippet` behavior. | This is a Cassie method. The virtual PostgreSQL catalog maps the full-text index to GIN's access-method OID/name for its selected descriptor shape; that mapping does not provide SQL GIN operator classes or GIN maintenance semantics. |
| `USING vector` | Cassie vector retrieval index over one `VECTOR(n)` field, with a required text/JSON `source_field`. | This is a Cassie method, not PostgreSQL's hash/GIN/GiST family or an extension ABI. Its metric and retrieval index type are Cassie options. |

`USING hash` is parsed so Cassie can return a deterministic unsupported-feature
error, then rejected by binding before catalog publication. Midge's ordered
key/value API does not establish PostgreSQL Hash semantics: PostgreSQL Hash
indexes store hash codes and support equality-only search. Cassie does not
claim that behavior. SQL `USING gin`, GiST, SP-GiST, and BRIN methods are
unsupported. Use `USING fulltext` for Cassie's full-text search. Other method
names are unsupported. The rejection regression for scalar Hash is
[`should_reject_hash_indexes_until_storage_supports_the_method`](../tests/compatibility/lifecycle/indexes/index.rs);
the selected-method rejection cases are in
[`selected_access_methods.rs`](../tests/compatibility/lifecycle/indexes/selected_access_methods.rs).

## Selected scalar btree syntax and options

The default method is btree. The selected row-authoritative scalar forms are:

- Primary and secondary indexes, multiple keys, unique keys, `NULLS
  [NOT] DISTINCT` on unique indexes, expressions, partial predicates, and
  `INCLUDE` fields.
- `CREATE INDEX IF NOT EXISTS` for a named index and generated names for
  unnamed `CREATE INDEX ON table (...)` forms.
- Per-key `ASC`/`DESC` and `NULLS FIRST`/`LAST`. Defaults are omitted from the
  rendered definition. The metadata does not change the Midge key comparator.
- The `text_pattern_ops` operator class on `TEXT`, `CHAR`, or `VARCHAR` field
  keys. Its optional `pg_catalog.` qualifier is normalized. It is recorded in
  the catalog but does not provide PostgreSQL pattern-operator acceleration.
- `WITH (fillfactor = n, deduplicate_items = bool)`, where `n` is 10 through
  100 and `bool` accepts `on`/`true`/`yes`/`1` or `off`/`false`/`no`/`0`.
  `ALTER INDEX ... SET/RESET` supports these same two parameters. Cassie stores
  and reports these options as metadata; they do not tune Midge index layout.

Other scalar operator classes and storage parameters are rejected. Scalar
`USING hash` is rejected. `INCLUDE`, ordered key modifiers, and operator
classes are selected only for scalar btree indexes; they are rejected for
FullText and Vector indexes. Unique and partial definitions are selected only
for scalar btree indexes. Cassie rejects `UNIQUE` and `WHERE` on FullText or
Vector indexes before publishing catalog or vector metadata; those index
builders do not implement uniqueness or predicate filtering.

Focused evidence already present in the repository includes:

- [`should_preserve_operator_class_with_mixed_btree_key_modifiers`](../tests/compatibility/lifecycle/indexes/operator_class_key_modifiers.rs)
  and [`should_reject_unselected_btree_operator_classes`](../tests/compatibility/lifecycle/indexes/operator_class_key_modifiers.rs).
- [`should_publish_partial_btree_index_clause_metadata`](../tests/compatibility/lifecycle/indexes/included_partial_indexes.rs)
  and [`should_publish_unique_index_nulls_not_distinct_metadata`](../tests/compatibility/lifecycle/indexes/nulls_not_distinct.rs).
- [`should_update_index_storage_parameter_catalog_state`](../tests/compatibility/lifecycle/indexes/storage_parameter_options.rs)
  and [`should_store_btree_deduplicate_items_option`](../tests/compatibility/lifecycle/indexes/storage_maintenance/semantics.rs).
- [`should_preserve_partial_index_results_after_reindex_with_null_keys`](../tests/compatibility/lifecycle/indexes/reindex_options.rs)
  and [`should_maintain_partial_index_entries_through_predicate_changing_writes`](../tests/compatibility/lifecycle/indexes/partial_index_query_equivalence.rs).

## Full-text and vector options

`USING fulltext (field)` accepts exactly one `TEXT` field and no expression or
`INCLUDE` key. Supported `WITH` keys are:

- `boost` (default `1`) and `k1` (default `1.2`): finite numbers greater than
  or equal to zero.
- `b`: finite number in `[0, 1]`; default `0.75`.
- `analyzer`: `standard` (default) or `simple`. `simple` defaults stop words to
  `none`; `standard` defaults them to `english`.
- `tokenizer`: `standard` (default) or `whitespace`.
- `case_folding`: Boolean, default `true`; `accent_folding`: Boolean, default
  `false`. Both accept `true`/`false`, `on`/`off`, or `1`/`0`.
- `stop_words`: `english` (default) or `none`.
- `stemming`: `none` (default).

Unknown keys, unsupported values, `UNIQUE`, partial predicates, multiple
fields, expressions, ordered modifiers, operator classes, and `INCLUDE` are
rejected. Unsupported clauses are tested by
[`should_reject_unique_fulltext_index_before_publication`](../tests/compatibility/lifecycle/indexes/selected_access_methods.rs)
and
[`should_reject_partial_fulltext_index_before_publication`](../tests/compatibility/lifecycle/indexes/selected_access_methods.rs);
full-text option parsing and validation cases are in
[`tests/parser_types/grammar/indexes/semantics.rs`](../tests/parser_types/grammar/indexes/semantics.rs).

`USING vector (vector_field)` accepts exactly one declared `VECTOR(n)` field
and requires `source_field` to name a `TEXT` or `JSON` field. `metric` defaults
to `cosine` and accepts `cosine`, `l2` (also `euclidean` and
`euclidean_distance`), or `dot` (also `dot_product`). `index_type` is
`bruteforce` (default), `hnsw`, or `ivfflat`.
The accepted type-specific options are:

| Index type | Options and defaults | Bounds |
| --- | --- | --- |
| `hnsw` | `m=16`, `ef_construction=64`, `ef_search=40` | `m` 2–128; `ef_construction` from `m` through 4096; `ef_search` 1–4096 |
| `ivfflat` | `lists=64`, `probes=1`, `training_sample_size=max(lists * 40, 1)`, `training_seed=1` | `lists` 1–65,536; `probes` 1–`lists`; `training_sample_size` from `lists` through 10,000,000; `training_seed` from 0 through the platform `usize` maximum |
| `bruteforce` | No type-specific options | HNSW and IVFFlat options are rejected |

Unknown options and options belonging to another index type are rejected.
`UNIQUE`, partial predicates, multiple fields, expressions, ordered modifiers,
operator classes, and `INCLUDE` are rejected. Unsupported clauses are tested by
[`should_reject_unique_vector_index_before_publication`](../tests/vector_embeddings/vectors/vector_index_drop.rs)
and
[`should_reject_partial_vector_index_before_publication`](../tests/vector_embeddings/vectors/vector_index_drop.rs);
the vector method and option cases are in
[`vector_indexes.rs`](../tests/vector_embeddings/vectors/vector_indexes.rs) and
[`ivfflat_indexes.rs`](../tests/vector_embeddings/indexes/ivfflat/ivfflat_indexes.rs).

## Drop and rebuild

`DROP INDEX [IF EXISTS] [schema.]name` drops an index without an `ON table`
clause. An explicit schema is exact; an unqualified name resolves the first
matching index in the session search path in the current database. This
resolution applies to all registered index kinds, including Cassie FullText
and Vector indexes. The legacy `DROP INDEX name ON table` form remains an
accepted extension. `DROP INDEX CONCURRENTLY` is unsupported.

`REINDEX INDEX name` and `REINDEX TABLE name` rebuild selected scalar btree
indexes from stored rows outside active transactions. Table rebuild is
rejected if the table contains a non-scalar index. The only accepted explicit
option set is `(VERBOSE FALSE, CONCURRENTLY FALSE)`. Schema, database, system,
concurrent, and verbose rebuilds are unsupported. REINDEX does not refresh
materialized rows. FullText and Vector indexes do not have a selected SQL
REINDEX path.

The scalar behavior is covered by
[`should_drop_only_the_schema_qualified_index_without_an_on_clause`](../tests/compatibility/lifecycle/indexes/index.rs),
[`should_drop_the_first_unqualified_index_in_search_path_order`](../tests/compatibility/lifecycle/indexes/drop_index_if_exists_scope.rs),
[`should_reindex_with_explicit_nonconcurrent_options_while_preserving_rows`](../tests/compatibility/lifecycle/indexes/reindex_options.rs),
and [`should_preserve_partial_index_results_after_reindex_with_null_keys`](../tests/compatibility/lifecycle/indexes/reindex_options.rs).
The FullText and Vector drop equivalence regressions added with this note are
[`should_preserve_fulltext_search_results_after_schema_qualified_drop`](../tests/search/index_lifecycle/fulltext_drop.rs)
and [`should_preserve_vector_top_k_results_after_schema_qualified_drop`](../tests/vector_embeddings/vectors/vector_index_drop.rs).

## Acceptance boundary

The v0.3 scalar-index lifecycle evidence in PR #1140 remains complete and is not
repeated here. This note records the v0.2 method and SQL DDL boundary plus the
remaining cross-kind drop and unsupported-clause evidence. No persistent
format, pre-release compatibility, or migration between development builds is
introduced by this contract.

This issue note does not qualify transactional publication or rollback of
index DDL; that evidence is owned by #833. `COMMENT ON INDEX` remains with
[#845](https://github.com/cntryl/cassie/issues/845), and column-authoritative
index lifecycle remains with [#1111](https://github.com/cntryl/cassie/issues/1111).
