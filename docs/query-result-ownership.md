# Query Result Ownership

This is the selected A+O implementation contract for
[#766](https://github.com/cntryl/cassie/issues/766), approved by the user on
2026-10-11. It records the source-breaking Rust API and budget/lifetime choices
before implementation. **Implementation and actual acceptance are pending.**
The canonical resource requirements remain in
[Performance Contracts](performance-contracts.md); delivery and closure follow
[PostgreSQL 18 Delivery](postgresql18-delivery.md).

The approved proposal SHA256 is
`92db6e81c902eedd1b891159e9903074bfc1bb43e0e30fff32f18f9491f77d3d`;
independent static review is
`a1c4a49c9be85564b3c7c9522e41991bb5dd4268f0beaa8bed20fa1aa9c49065`.
Its source baseline is the accepted [PR #933](https://github.com/cntryl/cassie/pull/933),
commit `ade53364ab2bea6d951d9ed9781b9a23ddae99f8`, tree
`3afa8cae42caa350cb85f9f003b718880d052d6a`. Static review establishes a
reviewable contract; it does not prove allocation, release or runtime behavior.

## Selected ownership and budgets

A replaces bare mutable result fields and deep Clone with immutable shared
owners. Engine-produced payloads and their capacity reservations remain coupled
through cache retention, public embedded return, REST conversion, owned-row
transfer and the final HTTP frame owner. Cheap Clone shares data and leases;
borrowed getters expose no mutable vectors or unguarded consuming Vec escape.
Host-created copies from borrowed views are host allocations, outside this
engine-allocation guarantee and any total-process RSS claim.

O keeps physical allocations charged to their producing query's shared tracker
until their last actual owner drops. This includes retained cache payload,
keys and backing containers after the producing query returns. A different query
must separately admit the complete exposed result footprint against its own
shared tracker before receiving a cache hit. Same-query aliases share admission;
producer physical ownership and requester exposure are distinct charges. A
completed producer can therefore retain nonzero live accounted bytes.

New conversion or encoding buffers use the managed view's **active admitted
query tracker**. After `try_clone_for_query`, those allocations belong to the
requester, never an unrelated cache producer. The old physical payload retains
its producer lease and the requester exposure remains admitted during overlap.
Direct same-query conversion uses the managed view's original tracker.

Owners retain only `QueryMemoryReservation` (tracker plus admitted bytes) and
their data. They never retain full `QueryExecutionControls`, statement-read
snapshots, cancellation, deadline, request or session scopes to keep a memory
lease alive. Controls are borrowed transiently for admission and the existing
cancellation/deadline frontiers. Narrow tracker capabilities may admit sibling
conversion buffers without retaining those other scopes.

Reserve before construction, growth, cloning or replacement. Account actual
backing capacity, nested values, metadata, strings, row/column directories,
keys, owner headers and simultaneous old/new allocation overlap. Map/deque
`clear()` does not retire their backing storage: its lease survives until
actual drop or shrink. Count a physically shared cache allocation once;
requester exposure admissions are separately reported query charges.

Required construction, replacement, conversion, encoding or hit admission fails
with existing SQLSTATE `54000` before denied allocation, hit publication or
partial success output. Existing cancellation/deadline priority remains.
Optional cache insertion may skip before extra key/container allocation when it
cannot fit; the valid independently admitted result remains successful and the
cache records `memory_budget` bypass. Failed requester admission records no
successful hit and leaves the valid cached owner available to a later query.

The current entry bound and serialized-payload
`CASSIE_EXECUTION_RESULT_CACHE_MAX_BYTES` semantics remain distinct from actual
capacity ownership. Keep `execution_result_cache.bytes` as serialized bytes;
add `execution_result_cache.accounted_retained_bytes` for retained capacity.
Live release observations follow the actual tracker/owners rather than stale
last-query snapshots. No cache-wide budget, runtime setting, format migration
or release-at-public-return exception is selected.

## Selected Rust result APIs

These signatures define the intended public surface; they are not a claim that
the current library implements them. Manual serialization preserves the existing
`columns`, `rows` and `command` shape. Construction and replacement are fallible;
private producers may transfer both already-admitted data and its reservation.

```rust
#[derive(Debug, Clone)]
pub struct QueryResult { /* private Arc-owned payload and admission owners */ }

impl QueryResult {
    pub fn columns(&self) -> &[ColumnMeta];
    pub fn rows(&self) -> &[Vec<Value>];
    pub fn command(&self) -> &str;
    pub fn row_count(&self) -> usize;

    pub fn try_builder(
        controls: &QueryExecutionControls,
    ) -> Result<QueryResultBuilder, CassieError>;

    pub fn try_clone_for_query(
        &self, controls: &QueryExecutionControls,
    ) -> Result<Self, CassieError>;

    pub fn try_with_columns(
        &self, controls: &QueryExecutionControls, columns: &[ColumnMeta],
    ) -> Result<Self, CassieError>;

    pub fn try_with_rows(
        &self, controls: &QueryExecutionControls, rows: &[Vec<Value>],
    ) -> Result<Self, CassieError>;

    pub fn try_with_command(
        &self, controls: &QueryExecutionControls, command: &str,
    ) -> Result<Self, CassieError>;

    pub fn into_owned_rows(self) -> OwnedQueryRows;
}

pub struct QueryResultBuilder { /* private accounted construction */ }
impl QueryResultBuilder {
    pub fn try_push_column(&mut self, column: &ColumnMeta) -> Result<(), CassieError>;
    pub fn try_push_row(&mut self, row: &[Value]) -> Result<(), CassieError>;
    pub fn try_set_command(&mut self, command: &str) -> Result<(), CassieError>;
    pub fn finish(self) -> Result<QueryResult, CassieError>;
}

#[derive(Debug, Clone)]
pub struct OwnedQueryRows { /* private payload/admission owners */ }
impl OwnedQueryRows {
    pub fn as_slice(&self) -> &[Vec<Value>];
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
}
```

`try_with_*` preserves the source on failure and admits any replacement copy
before allocation. `try_clone_for_query` admits a new query's exposure before
returning a view. `OwnedQueryRows` retains its owners while slices are borrowed,
including across async writes. Existing public runtime cache lookup/store names
and return types may remain once the returned QueryResult itself is managed;
SQL uses a private controlled `Result<Option<QueryResult>, CassieError>` lookup
before hit publication. Direct cache facade calls retain an existing managed
owner and do not create an implicit SQL query context.

## Selected REST and EXPLAIN APIs

Public REST/explain result types also become managed. Public field literals,
field moves and mutation migrate to borrowed getters, fallible builders and
owner-preserving transfers. Structured-plan builders admit nested capacity;
engine code cannot return an unowned nested plan clone. Nested plan data may
remain borrowed views with their existing types.

```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct RestQueryResult { /* private immutable payload and narrow admission owners */ }
impl RestQueryResult {
    pub fn columns(&self) -> &[RestColumnMeta];
    pub fn rows(&self) -> &[Vec<serde_json::Value>];
    pub fn command(&self) -> &str;
    pub fn row_count(&self) -> usize;
    pub fn try_from_result_with_controls(
        result: QueryResult, controls: &QueryExecutionControls,
    ) -> Result<Self, CassieError>;
}
impl TryFrom<QueryResult> for RestQueryResult {
    type Error = CassieError;
    fn try_from(result: QueryResult) -> Result<Self, Self::Error>;
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct QueryExplainPlan { /* private immutable structured-plan payload and narrow owners */ }
impl QueryExplainPlan {
    pub fn format_version(&self) -> u32;
    pub fn summary(&self) -> &QueryPlanSummary;
    pub fn nodes(&self) -> &[QueryPlanNode];
    pub fn attributes(&self) -> &[QueryPlanAttribute];
    pub fn estimates(&self) -> &QueryPlanEstimates;
    pub fn features(&self) -> &[QueryPlanFeature];
    pub fn diagnostics(&self) -> &QueryPlanDiagnostics;
    pub fn analyze(&self) -> Option<&QueryPlanAnalyze>;
}
#[derive(Debug, Clone, serde::Serialize)]
pub struct QueryExplainOutput { /* private managed result and managed plan */ }
impl QueryExplainOutput {
    pub fn result(&self) -> &QueryResult;
    pub fn plan(&self) -> &QueryExplainPlan;
    pub fn into_parts(self) -> (QueryResult, QueryExplainPlan);
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RestQueryExplainResponse { /* private managed REST result and managed plan */ }
impl RestQueryExplainResponse {
    pub fn columns(&self) -> &[RestColumnMeta];
    pub fn rows(&self) -> &[Vec<serde_json::Value>];
    pub fn command(&self) -> &str;
    pub fn plan(&self) -> &QueryExplainPlan;
    pub fn try_from_output_with_controls(
        output: QueryExplainOutput, controls: &QueryExecutionControls,
    ) -> Result<Self, CassieError>;
}
impl TryFrom<QueryExplainOutput> for RestQueryExplainResponse {
    type Error = CassieError;
    fn try_from(output: QueryExplainOutput) -> Result<Self, Self::Error>;
}
```

The existing public `query::execute`,
`query::execute_with_session_and_cancellation`, `query::explain` and
`search::vector_search` names, arguments and return type names remain. Their
results retain ownership after return. Public TryFrom consumes a managed source
and admits new conversion buffers through its active narrow tracker; it creates
no new query context. Internal SQL/HTTP callers use controlled conversion while
live request controls still supply cancellation/deadline checks. REST JSON keeps
its existing flattened `columns`, `rows`, `command`, and explain `plan` fields.

SQL query HTTP encoding is fallible and completes admission before publishing
success headers or a partial body. Private controlled query/explain response
helpers execute, convert and encode under the same live controls before finishing
the operation. They return an owned prepared response, never retained controls.
They cannot use an unaccounted `serde_json::to_vec` or `{}` fallback on denial.
Existing resource-error mapping handles the failed response.

```rust
pub(crate) fn try_query_json_response(
    status: StatusCode, value: &RestQueryResult, controls: &QueryExecutionControls,
) -> Result<Response<RestBody>, CassieError>;
pub(crate) fn try_explain_json_response(
    status: StatusCode, value: &RestQueryExplainResponse, controls: &QueryExecutionControls,
) -> Result<Response<RestBody>, CassieError>;
```

An admitted private byte owner transfers buffer plus reservation into
`Bytes::from_owner` (available in pinned bytes 1.12.0), preserving the private
`RestBody` type. A checked writer reserves growth and the ownership header before
allocation. The owner survives body/frame/Bytes clones: dropping a body cannot
release a buffer still held by a polled frame. Final frame retirement, disconnect,
conversion failure and cancellation release their actual owners exactly once.

## Implementation and acceptance boundary

Migrate existing result producers, cache storage/lookup, pgwire writers and
materialized portals, REST query/search conversion, structured EXPLAIN output,
HTTP encoding, actual external-style fixtures and benchmark consumers. Preserve
SQL values, descriptors/OIDs, tags, ordering, JSON/plan format, Midge architecture
and persistent identities. An initial shared portal plus row offset may retain
emitted prefix storage; charge its complete actual capacity. This can reach a
memory limit earlier than draining rows. Any suffix copy must first admit the
old/new overlap.

Use focused modules below their size limits and small tests-first local commits.
Actual acceptance must prove:

- final shared-owner release with variable-width rows, JSON, vectors and metadata;
- exact-fitting and one-byte-short rejection before growth, without partial output;
- owner-preserving consuming row transfers and same-query cheap Clone;
- optional insertion skip and real smaller-budget warm-hit rejection;
- replacement/eviction/invalidation release including actual map/deque capacity;
- public REST/TryFrom/EXPLAIN return ownership and conversion-error cleanup;
- encoded execute/explain bytes held after body drop until final frame drop;
- statement-read, cancellation and request scopes released while narrow leases remain;
- unchanged simple/extended pgwire, REST, portal and cache/authentication controls.

Keep accepted #933 context, publication/restart, transaction/savepoint, persisted
key and authority evidence. This selected bundle adds no SQL type, AST, durable
format, transaction-error API or permission model. Schema/validate/admin DTOs
outside execution-result-cache payloads retain their separate wider resource
obligations. #766 stays open until the complete current-surface contract passes
actual tests, the ordered gate, adversarial review/probes, required GitHub rules,
squash and merged source/issue readback. Full #904/#793/#791/#848/#849 remain open;
the original parameter occurrence/coercion gaps are outside this bundle.
