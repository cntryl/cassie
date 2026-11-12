# `ARRAY_AGG(anyarray)` contract for #811

This finite rule extends the selected PostgreSQL array-shape contract in
[`type-contract.md`](type-contract.md#selected-postgresql-array-shape-contract-2026-10-26)
for the ordered collection-aggregate work in [#811](https://github.com/cntryl/cassie/issues/811).
It does not select broader PostgreSQL aggregate or array parity.

## Selected behavior

`ARRAY_AGG(anynonarray)` collects its inputs, including SQL NULL, as elements
of a one-dimensional result. `ARRAY_AGG(anyarray)` uses PostgreSQL's array
input overload: it builds an array with one additional outer dimension. Its
selected input arrays must be non-NULL, nonempty, and have identical dimension
lengths and lower bounds. The outer dimension has lower bound 1; each input
array's shape and lower bounds become the remaining dimensions.

The anyarray overload rejects a selected SQL NULL input with SQLSTATE `22004`
and message `cannot accumulate null arrays`. It rejects an empty input array
or incompatible input shapes with SQLSTATE `2202E`, using PostgreSQL's messages
`cannot accumulate empty arrays` and `cannot accumulate arrays of different dimensionality`,
respectively. Aggregate `FILTER` is applied before this validation. Existing
`DISTINCT` and aggregate-local `ORDER BY` behavior follows the selected #804
modifier contract.

This distinction is based on the bound argument type. A NULL `anynonarray`
value remains a valid array element; a NULL value bound as an array input is an
error.

## PostgreSQL 18.6 oracle

The local PostgreSQL server reported version 18.6. These exact statements
returned the following errors:

| Input | SQLSTATE | Message |
| --- | --- | --- |
| `ARRAY[1,2]::INT[]` followed by `NULL::INT[]` | `22004` | `cannot accumulate null arrays` |
| `ARRAY[]::INT[]` | `2202E` | `cannot accumulate empty arrays` |
| `ARRAY[1,2]::INT[]` and `ARRAY[3,4,5]::INT[]` | `2202E` | `cannot accumulate arrays of different dimensionality` |
| `array_fill(1, ARRAY[2], ARRAY[0])` and `ARRAY[3,4]` | `2202E` | `cannot accumulate arrays of different dimensionality` |

PostgreSQL 18's [aggregate reference](https://www.postgresql.org/docs/18/functions-aggregate.html)
specifies that the array-input overload requires equal dimensionality and does
not accept empty or NULL arrays. The local oracle additionally confirms the
exact errors and that lower-bound differences are rejected by this overload.

## Current Cassie evidence and typed NULL root cause

The anyarray transition in
`src/executor/execution/aggregate_exec/state.rs` now validates selected
SQL NULL, empty, and incompatible-shape inputs with the selected PostgreSQL
SQLSTATEs and messages. FILTER is evaluated before the transition. The direct
value and FILTER regressions are authored in
[`aggregate_semantic_edges.rs`](../tests/sql_queries/aggregates/arrays/aggregate_semantic_edges.rs).

The pgwire typed-NULL bypass came from dropping declared Parse OIDs after
parameter admission. `src/app/query_bound.rs` retained the admitted OID slice
for planning and descriptors but passed only runtime `Value`s into execution.
`src/executor/array_order.rs::expression_type` then called
`infer_expr_type(..., &[])`, so `Expr::Param` inferred as `DataType::Null` even
when Parse declared OID 1007 (`INT4[]`). In the modifier path,
`ModifierAccumulator::push_row` applies `Seen::insert` before the aggregate
transition. The untyped NULL marker therefore treated the argument as
`anynonarray`; DISTINCT collapsed repeated NULLs and the anyarray validator
never ran. `ARRAY_AGG(anynonarray)` must continue to accept NULL elements.

The SELECT implementation carries admitted parameter OIDs from bound-query
execution through the plan, source, EXISTS, CTE, and aggregate execution
contexts. Aggregate input classification, DISTINCT keys, and result-type
inference use that slice. Non-pgwire and rollup entry points retain empty OID
slices. The selected source choice is the Jev result
`thread_declared_parameter_oids` (jev-1.14.0, confidence 1.0); the precise
root-cause trace and action are recorded alongside that request and response
under `target/v020-evidence/811-array-aggregate/typed-null-root-cause.md`.

[`array_aggregate.rs`](../tests/pgwire_extended/aggregates/arrays/array_aggregate.rs)
adds an end-to-end Parse OID 1007 plus NULL Bind regression named
`should_reject_typed_null_array_inputs_before_distinct_aggregation`. It covers
the ordinary transition, DISTINCT, aggregate-local ORDER BY, and
FILTER selected/excluded variants. It asserts SQLSTATE `22004` for selected NULL array inputs and a
NULL aggregate result when FILTER excludes the input. The probes are authored but remain unexecuted
pending the coordinator's bundle gates. JSON and JSONB collection runtime work
remains blocked by the open #810 prerequisite; this contract does not change
that boundary.

## Focused DISTINCT review

Jev's path probe assigned probability 0.38 to a DISTINCT shape-validation gap,
above the focused-probe threshold. The follow-up question
`distinct_suppresses_invalid_array` returned 0.23. Source inspection confirms
that `SemanticValue::from_value(Value::Array)` includes every dimension's
length and lower bound, so equal flat elements with a changed lower bound do
not collapse to one DISTINCT key. The focused regression
`should_validate_distinct_array_inputs_with_different_lower_bounds` exercises
that case and expects SQLSTATE `2202E`; it is authored/not run. The question and
typed result are preserved in
[`distinct-bypass-focused-request.json`](../target/v020-evidence/811-array-aggregate/distinct-bypass-focused-request.json),
[`distinct-bypass-focused-response.json`](../target/v020-evidence/811-array-aggregate/distinct-bypass-focused-response.json),
[`validation-gap-path-probes-response.json`](../target/v020-evidence/811-array-aggregate/validation-gap-path-probes-response.json),
and
[`validation-gap-focused-followup-response.json`](../target/v020-evidence/811-array-aggregate/validation-gap-focused-followup-response.json).

## Bound EXPLAIN and DML routes

Bound `EXPLAIN ANALYZE` executes the bound physical SELECT. Its previous helper
call dropped the admitted OIDs, so the typed-NULL regression now includes an
`EXPLAIN ANALYZE` case and execution uses
`execute_physical_statement_with_parameter_oids`. That case is authored/not
run.

The source audit also found a separate DML OID-drop route. In
`entrypoints.rs::run_with_session_parameter_oids`, the command branch calls
`dml_command::execute_command` without the OID slice. That command passes only
runtime values to `dml_insert::execute_insert`. For `InsertSource::Select`,
`dml_insert.rs::insert_source_rows` builds a logical plan and calls
`dispatch::execute_plan`, whose wrapper supplies an empty OID slice. UPDATE row
subqueries and joined UPDATE/DELETE/MERGE predicates also create empty-OID
contexts through `dml_subqueries::RowSubqueries` and
`dispatch::resolve_statement_exists`.

The representative unexecuted wire case is:

```sql
CREATE TABLE issue_811_typed_null_target (value INT4[]);
INSERT INTO issue_811_typed_null_target
SELECT array_agg($1) FROM (VALUES (1),(2)) AS t(n);
```

With Parse OID 1007 and Bind NULL, the selected aggregate contract expects
SQLSTATE `22004`. Jev judged that this route can lose the OID and bypass the
validator at probability 0.87, while its scope choice selected `select_only`
at probability 0.89. The requests and typed results are preserved in
[`dml-oid-scope-request.json`](../target/v020-evidence/811-array-aggregate/dml-oid-scope-request.json)
and
[`dml-oid-scope-response.json`](../target/v020-evidence/811-array-aggregate/dml-oid-scope-response.json).
Per that selected scope, this slice covers SELECT and bound `EXPLAIN ANALYZE`
only. It does not alter or qualify the DML command pipeline; the DML route is
outside this slice's acceptance evidence.
