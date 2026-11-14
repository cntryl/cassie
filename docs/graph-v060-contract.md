# v0.6 graph contract and delivery plan

Status: **selected planning direction, not implemented or qualified**. This
document does not claim SQL:2023 conformance. Reconcile live issue wording with
these decisions before implementation. Normative verification, implementation
feasibility and the storage proof remain gates. Existing graph traversal remains
Experimental.

## Outcome

A projector maintains ordinary durable vertex and edge projection tables. An
application using a PostgreSQL driver declares a graph over those tables and
uses graph patterns together with SQL to answer relationship, hierarchy and
path questions. Results reflect the query's transaction view, have documented
semantics and bounded resource behavior, and remain correct across writes,
restart and derived-index damage. A neighborhood lookup should scale with the
selected neighborhood, not require a full graph verification on every hop.

This is a single-node read-model feature on Midge. It is not a graph database
cluster, full SQL/PGQ or Cypher, a second storage engine, graph mutation syntax,
or a general graph analytics platform.

## Graph definition and identity

The proposed `CREATE PROPERTY GRAPH` maps **existing** tables; creating or
dropping a graph does not create or delete source rows or tables. A definition
names vertex table aliases, unique vertex keys, edge table aliases, unique edge
keys, source and destination key mappings, labels and exposed properties.
Multiple tables and multiple labels are required for realistic typed read
models. Parallel edges and self-loops are distinct when edge keys differ.
All mapped tables belong to one Cassie database and its Data family. Both
row-authoritative and column-authoritative sources must return exact results;
where native adjacency cannot be verified for a source, use the controlled
authoritative fallback. A table mapped by multiple graphs or aliases requires
correct atomic fan-out to every affected derived index; `graph_for_edge_collection`
currently finds only the first graph and is not evidence of that capability.

Each key is a non-null, declared key tuple backed by a globally enforcing,
non-partial unique constraint or unique index on precisely that ordered tuple;
a partial unique index is insufficient. Graph definition verifies each key
component is non-NULL and prevents dropping or changing its enforcing key while
referenced unless the graph dependency is removed by an explicitly supported
cascade. Endpoint keys
have the same arity and corresponding SQL types as the referenced vertex key.
Tuple equality uses the same SQL equality semantics as the underlying key type;
identifier folding follows Cassie's ordinary SQL identifier rules. An element's
identity is its durable database, graph, element-kind and mapping identity plus
its complete typed key, not a display alias, label or stringified key. The
eligible types and collation are specified below. Floating-point keys and
other types without a proven identity and total order are rejected.

An edge participates only when both mapped vertices
exist and all endpoint key components are non-null in the query snapshot; a
dangling edge remains an ordinary table row and becomes visible when its
endpoint is inserted. Graph mapping imposes no new foreign key or cascading
delete on projector tables. This is Cassie's selected read-model policy, not a
claim about SQL/PGQ. Vertex and edge property
values, endpoint resolution and adjacency must be read in the same transaction
view; own inserts, updates, deletes and savepoint rollback must be visible.

Referenced table, key and column lifecycle must be defined mechanically:
reject incompatible drop/type change while referenced unless an explicit
supported cascade removes the graph metadata; drop graph leaves tables intact.
Rename follows stable catalog identity and persists through restart. Access
requires authority to read all referenced tables and exposed properties; a
graph must not bypass underlying permissions. Catalog discovery must honor the
same access boundary.

The selected finite definition and lifecycle grammar is below. This is Cassie's
contract, not a claim of ISO conformance. `qualified_name` is an optionally
schema-qualified SQL identifier; lists contain one or more comma-separated
entries; square brackets mean optional and braces mean repetition:

```text
create_graph = CREATE PROPERTY GRAPH qualified_name
               VERTEX TABLES (vertex_mapping {, vertex_mapping})
               EDGE TABLES (edge_mapping {, edge_mapping})
vertex_mapping = qualified_table AS alias KEY (column {, column}) label_spec {label_spec}
edge_mapping = qualified_table AS alias KEY (column {, column})
               SOURCE KEY (column {, column}) REFERENCES vertex_alias (column {, column})
               DESTINATION KEY (column {, column}) REFERENCES vertex_alias (column {, column})
               label_spec {label_spec}
label_spec = LABEL label PROPERTIES (property {, property})
property = column [AS property_name]
drop_graph = DROP PROPERTY GRAPH [IF EXISTS] qualified_name
rename_graph = ALTER PROPERTY GRAPH qualified_name RENAME TO new_name
```

All names use Cassie's SQL identifier quoting and folding rules. Resolve the
graph name and each source table in the current database using ordinary schema
resolution; graph rename changes only its name within the same schema. Mapping
aliases are required and unique within their element kind; a vertex and an
edge may share an alias. Each mapping must declare at least one label; a mapping
may declare several distinct labels and each label declares its exposed
properties. A property without `AS` keeps the column name. Each property name
is unique within its label definition; repeated labels across mappings select
all matching mappings. A selected label exposes a property only when every
selected mapping exposes it with the same SQL type; otherwise binding fails.
Endpoints reference vertex aliases, and their referenced column tuples must
exactly match that alias's ordered `KEY`; corresponding source/destination
columns must have identical SQL types and arity. Sources must be ordinary
row- or column-authoritative tables in the same database; views and
cross-database references are unsupported in v0.6.

Create rejects an existing graph (`42710`), missing schema (`3F000`), source
table (`42P01`) or column (`42703`), duplicate mapping alias within its element
kind, duplicate label within one mapping, or duplicate exposed property name
within one label definition (`42710`), and incompatible endpoint types
(`42804`). The same label may occur in distinct mappings. Reject a
missing globally enforced unique key, nullable key or wrong endpoint key tuple
with `42830`; unsupported key type, collation, source kind or syntax feature
returns `0A000`. Malformed syntax returns `42601`. Drop or rename of a missing
graph returns `42704`, except `DROP ... IF EXISTS` succeeds with a notice.
Rename to an existing graph returns `42710`. Graph DDL is atomic with catalog
publication; graph drop never deletes source tables or rows. Referenced source
objects and key constraints reject incompatible changes with `2BP01`; if
`CASCADE` would be required to remove a graph, return `0A000` until explicitly
implemented. Source schema/table/column renames preserve resolved identities and
update displayed metadata atomically. Cross-schema graph moves, graph-definition
alteration beyond rename, and graph mutation syntax return `0A000`.

For example:

```sql
CREATE PROPERTY GRAPH social
  VERTEX TABLES (
    people AS person KEY (id) LABEL person PROPERTIES (id, name)
  )
  EDGE TABLES (
    follows AS knows KEY (id)
      SOURCE KEY (follower_id) REFERENCES person (id)
      DESTINATION KEY (followee_id) REFERENCES person (id)
      LABEL knows PROPERTIES (active, cost)
  );
```

Each mapping has a unique alias within its element kind. Shared labels expose a
property only when its type is compatible across all mappings selected by that
label; otherwise binding that property fails. Undefined tables or columns,
non-unique or nullable keys, incompatible endpoints, and unsupported key
types fail graph creation atomically. A graph over existing populated tables
must publish its definition only after an initial consistent backfill succeeds.
Fence concurrent writers for backfill and publication, or prove a dual-write
and catch-up protocol with atomic visibility. Synchronized writer/backfill
tests must cover inserts, updates, deletes, failure, cancellation and restart;
no published graph may expose an incomplete derived state.

## Query surface

The target is a finite `GRAPH_TABLE` pattern surface, composable with `SELECT`,
joins, filters and PostgreSQL-driver prepared parameters. The following is the
selected Cassie grammar, not a claim of ISO conformance. Square brackets in
this grammar denote optional parts, braces denote repetition, and quoted
brackets in `edge` are SQL tokens:

```text
graph_table = GRAPH_TABLE ( graph_name MATCH [path_variable =] path
                           [KEEP selector] [WHERE sql_predicate]
                           COLUMNS (projection {, projection}) ) [AS alias]
path        = vertex {edge [quantifier] vertex}
vertex      = ( [variable] [IS label {| label}] [WHERE sql_predicate] )
edge        = - [ [variable] [IS label {| label}] [WHERE sql_predicate] ] ->
            | <- [ [variable] [IS label {| label}] [WHERE sql_predicate] ] -
            | - [ [variable] [IS label {| label}] [WHERE sql_predicate] ] -
quantifier  = '{' n '}' | '{' m ',' n '}' | '*' | '+'
selector    = ALL [mode] PATHS | ANY SHORTEST [mode] PATH
            | SHORTEST k [mode] PATHS
mode        = WALK | TRAIL | ACYCLIC | SIMPLE
projection  = sql_expression [AS column_alias]
```

Numbers `m`, `n`, and `k` are unsigned integer literals; `m <= n` and `k > 0`.
`{n}` is exactly n, `{m,n}` is inclusive, `*` is zero or more, and `+` is one
or more. An absent quantifier means exactly one edge. An omitted selector means
`ALL WALK PATHS`; an omitted mode means `WALK`. Parenthesized multi-edge
repetition, nested quantifiers, selector parameters, and other quantifier forms
are outside this initial grammar. The query examples describe intended behavior,
not an implementation currently available:

```sql
SELECT g.person_id, g.friend_id
FROM GRAPH_TABLE (
  social MATCH (a IS person) -[e IS knows WHERE e.active = TRUE]->{1,3} (b IS person)
  WHERE a.id = $1
  COLUMNS (a.id AS person_id, b.id AS friend_id)
) AS g;
```

One path pattern may be a vertex alone or a chain of fixed and quantified edges,
including different labels and directions. A repeated singleton variable means
the same graph element at each occurrence; its labels and inline predicates all
apply. A variable cannot name both a vertex and an edge. A path variable is
distinct from element variables. Labels select mappings; properties exposed by
the selected mappings must bind to one compatible SQL type or binding fails.
Bound values are parameters, not SQL text interpolation. One matching path
yields one row; parallel edges and distinct matching paths retain multiplicity.
No result order is promised without `ORDER BY`, except the documented choice
among tied paths when a selector explicitly limits the result.

A quantified edge variable is a group of its ordered edge bindings, even for
`{1,1}` or an empty match. Its inline `WHERE` is evaluated once for each edge
before selecting paths. Outside that inline scope, direct scalar references such
as `e.active` are unsupported for a group variable: use the native path and
ordered step expansion. Group-variable horizontal aggregation is a separate
building block, and is not implied by ordinary SQL aggregation. Unquantified
edge and vertex variables are singleton bindings. Expressions must respect SQL
types and three-valued logic; only `TRUE` satisfies a predicate.

For a zero-edge path the start and end refer to the same existing graph vertex
identity and the edge sequence is empty. That vertex must satisfy the labels
and inline predicates of both declared endpoint patterns. Across repetitions,
adjacent edge endpoints must resolve to the same graph vertex identity;
declared vertex labels and predicates constrain their declared endpoints, not
every implicit intermediate vertex. Each repeated edge still satisfies its own
label and inline predicate. A nonmatching pattern yields no row. SQL NULL and
three-valued predicate semantics apply. For an undirected traversal, a
self-loop is one matching edge step, not two duplicate orientations. A
graph-pattern `WHERE` applies to selected complete paths, after a `KEEP`
selector. To constrain the search itself, place an edge predicate inline. An
outer SQL `WHERE` is also applied to rows after `GRAPH_TABLE` produces them.
For selective anchors, use an inline vertex predicate so it constrains the
search before selection. A planner may push an outer predicate into traversal
only after proving equivalence for selection, ties, NULLs, multiplicity,
errors and resource controls.

Recognized but unsupported graph constructs, including comma-separated
patterns, pattern unions, graph mutation, direct scalar use of group variables,
and unsupported quantifiers, return SQLSTATE `0A000`; malformed syntax returns
`42601`. Invalid bounds or a zero `k` return `22023`. Unknown variables or
properties and incompatible types use Cassie's corresponding existing binder
errors. Errors must not silently select a different meaning.

### Path modes and selectors

The selected semantics are:

* `WALK` allows repeated vertices and edges. `TRAIL` disallows repeated edge
  identities. `ACYCLIC` disallows repeated vertex identities. `SIMPLE` allows
  the first vertex to recur only as the final vertex, with no other vertex
  repetition. Verify grammar against the normative reference before claiming
  SQL conformance.
* A quantified edge has an inclusive lower and upper hop count. Unbounded enumeration of
  unrestricted walks is rejected; finite `TRAIL`/`ACYCLIC`/`SIMPLE` matches
  may still be exponentially numerous and remain subject to query limits.
* Reject an unbounded `WALK` even with a selector until finite semantics and a
  terminating implementation are established. An unreachable endpoint in a
  cyclic graph is a required counterexample: resource limits do not prove
  termination. Hop-shortest selectors rank paths satisfying the path structure,
  bounds, labels, path mode and inline predicates by hop count, separately for
  each bound endpoint pair. `SHORTEST k` returns up to k individual paths and
  `ANY SHORTEST` returns one per pair, before the graph-pattern `WHERE` filters
  them. `ORDER BY` is still needed to order result rows.
* Ties must use a documented total order on stable typed vertex and edge
  identities, not storage iteration order. Compare paths lexicographically by
  their ordered sequences of vertices and edges: element references order by
  durable database ID, graph ID, kind, mapping ID and key tuple; key components
  order first by declared type (smallint, integer, bigint, text, uuid), then by
  that type's value (C byte order for text and unsigned 16-byte order for UUID).
  Compare direction after each edge identity, forward before reverse, and the
  shorter sequence first when one is a prefix. These identity IDs and ordering
  must survive restart. Do not prune solely by visited node
  when path mode, a hop bound, filters or requested multiplicity requires
  retaining alternative histories. Compare results with independent exhaustive
  small-graph oracles.

Oracle's
[GRAPH_TABLE reference](https://docs.oracle.com/en/database/oracle/oracle-database/26/sqlrf/graph_table-operator.html)
and [graph pattern reference](https://docs.oracle.com/en/database/oracle/oracle-database/26/sqlrf/graph-pattern.html)
are useful implementation references but are not a substitute for the SQL
standard. Do not describe a vendor dialect or this selected grammar as verified
standard grammar. Compare normative grammar before claiming conformance.

Required independent oracle cases include:

* Direct `A -> B` with inactive edge and active `A -> C -> B`: inline
  `active` plus shortest selects the two-hop path; selecting first and applying
  a post-selection predicate to its path rejects the one-hop result and does
  not substitute the longer path.
* With `A -> B` and `A -> C -> B`, bounds `{2,3}` select the two-hop path;
  zero lower bound binds both endpoints to the same existing vertex.
* In `A -> B -> A`, `SIMPLE` accepts the closing cycle and `ACYCLIC` rejects
  it. An undirected self-loop produces one step, and parallel edges produce
  distinct matches. Quantified `{1,1}` still binds a group variable.
* Cyclic unbounded `WALK` with an unreachable destination fails validation,
  including with a shortest selector; it cannot run until a memory limit is hit.

### Paths and weighted queries

The selected path carrier uses Cassie-owned **named PostgreSQL composite
types** in the reserved `cassie_graph` schema. Its ordered public fields are:

```sql
cassie_graph.graph_key_part (
  type_tag smallint, integer_value bigint, text_value text, uuid_value uuid
)
cassie_graph.graph_element_ref (
  database_id uuid, graph_id uuid, mapping_id uuid, element_kind smallint,
  key_parts cassie_graph.graph_key_part[]
)
cassie_graph.graph_path (
  vertices cassie_graph.graph_element_ref[],
  edges cassie_graph.graph_element_ref[], directions smallint[]
)
```

`type_tag` values 1–5 represent `smallint`, `integer`, `bigint`, `text`, and
`uuid` respectively. For tags 1–3 only `integer_value` is non-NULL and must
fit the tagged SQL type; for 4 only `text_value` is non-NULL; for 5 only
`uuid_value` is non-NULL. `element_kind` is 1 for vertex and 2 for edge;
direction is 1 for source-to-destination and 2 for reverse. All other tags,
NULL mandatory fields, extra populated value slots and mismatched kinds reject.
Nonempty arrays are one-dimensional with lower bound 1; empty arrays use the
canonical PostgreSQL empty-array representation. Array elements are non-NULL. A key
tuple is nonempty. Vertex count equals edge count plus one and direction count
equals edge count. Array order is path order. A zero-hop path has one vertex
and empty edge and direction arrays. The typed key and durable mapping identity
identify an element; display labels do not.

For v0.6 key components are non-null `smallint`, `integer`, `bigint`, `text`
with C collation, or `uuid`, including ordered tuples of these types. Endpoint
components have the same declared types and order as their referenced key.
Other types, collations, nullable keys and coercion-based endpoint matches are
rejected at graph definition, not silently stringified. Integer range, exact
text and UUID semantics match their SQL types; equality and tie-breaking must
agree with those semantics. Adding other key families requires an equivalent
identity, ordering and index proof.

Database, graph and mapping IDs are durable UUID logical identities, rather
than SQL names or type OIDs: rename preserves them; a dropped and recreated object
gets a new identity. An operation resolving a reference from a different
database or a removed graph/mapping fails deterministically instead of
retargeting it. Export, restore and cloning must document identity handling;
do not promise cross-database reference portability. A missing durable allocator
for database, graph and mapping identities is an explicit #1268 prerequisite;
these identities are a target contract, not a claim about today's catalog. Public labels and
properties are resolved from the applicable graph snapshot; identity equality
does not depend on the current label text. References are not authorization
tokens and must not bypass source-table access checks.

Expose a relation of ordered path steps with zero-based position, direction,
vertex reference and incoming edge reference (both direction and edge NULL on
the first step). Weighted results retain an ordered per-edge cost sequence
alongside the generic path; it is not part of `graph_path`. Its length must
equal the path edge count. The weighted step relation takes both the path and
this cost sequence and derives cumulative cost from it, with zero at the first
vertex. Account for the
sequence before retaining it and release the reservation with the result. Provide typed
key-part expansion with ordinal, type discriminator and typed value columns so
ordinary PostgreSQL drivers can join projected key values without registering a
custom decoder. Also provide a versioned, lossless JSONB export for clients that
want an interchange representation: include graph/mapping identity, kind,
ordered typed key parts and direction; encode integer values as decimal strings
so JSON consumers cannot round BIGINT values. JSON is an export, not the native
identity or equality mechanism.

The system owns the reserved schema and its types. Bootstrap must inspect the
existing catalog first: if `cassie_graph` already exists without the recognized
system ownership marker, fail with `42710` and an actionable diagnostic,
preserving all existing objects, ownership and permissions. Never take over or
overwrite user objects. Operators resolve the name conflict through their
supported schema migration workflow before enabling the feature. Qualification
must exercise an existing user-owned schema, failed bootstrap, restart and a
successful retry after resolution. User DDL must not alter,
rename, replace or drop them, including by cascade. Bootstrap the schema,
composite descriptors, type and array OIDs and backing composite relation OIDs
atomically before publishing the graph feature; failed bootstrap leaves none
partially visible. Use the database-scoped `Midge::allocate_object_id` precedent
for named composite type, array and `relkind = 'c'` relation OIDs; do not use
OIDs as the durable graph identities above. Type and array OIDs and their fields
must be discoverable through `pg_type`, `pg_class` and `pg_attribute`. They
remain stable across restart for the life of the database, but clients must
discover them rather than assume fixed OIDs. Image restore and clone must define
and test OID and logical-identity handling before cross-database use; an old
reference must never silently resolve to a different mapping.
Use PostgreSQL composite and array text and binary framing, including nested
quoting, element type OIDs, NULLs and array bounds. Prepared statements and
result metadata must preserve the same types. Some drivers require explicit
registration to decode named composite arrays; the scalar expansion interface
must work without it. PostgreSQL composite types cannot themselves enforce the
tagged-part invariant: constructors and text/binary input paths must validate
it, and every graph-consuming API, equality operation, and step expansion must
validate shape, kind and reference freshness regardless of construction path.
Malformed or stale values reject deterministically, including values produced
by SQL `ROW` casts or subsequent array/field mutation. Tests cover those
construction paths as well as direct parameters.

The selected SQL APIs are schema-qualified and return the named types above:

```text
cassie_graph.key_part(value smallint|integer|bigint|text|uuid)
    -> cassie_graph.graph_key_part
cassie_graph.vertex_ref(graph text, mapping text, key_parts cassie_graph.graph_key_part[])
    -> cassie_graph.graph_element_ref
cassie_graph.edge_ref(graph text, mapping text, key_parts cassie_graph.graph_key_part[])
    -> cassie_graph.graph_element_ref
cassie_graph.key_parts(ref cassie_graph.graph_element_ref)
    -> TABLE (ordinal bigint, type_tag smallint, integer_value bigint,
              text_value text, uuid_value uuid)
cassie_graph.path_steps(path cassie_graph.graph_path)
    -> TABLE (position bigint, direction smallint,
              vertex cassie_graph.graph_element_ref, edge cassie_graph.graph_element_ref)
cassie_graph.weighted_path_steps(path cassie_graph.graph_path, edge_costs numeric[])
    -> TABLE (position bigint, direction smallint,
              vertex cassie_graph.graph_element_ref, edge cassie_graph.graph_element_ref,
              cumulative_cost numeric)
cassie_graph.path_to_jsonb(path cassie_graph.graph_path) -> jsonb
```

`key_part` is overloaded on the five listed exact input types and rejects NULL;
no implicit conversion changes the type tag. `graph` is an optionally
schema-qualified graph name resolved in the current database; `mapping` is its
mapping alias in the requested element kind. Constructors resolve the typed
tuple in that mapping's current transaction view, check key arity and exact
SQL types and require the element to exist and be readable. NULL required
arguments return `22004`; wrong type or arity returns `42804`; unknown graph,
mapping or element returns a documented deterministic missing-object error.
`key_parts` ordinal starts at one; `path_steps` position starts at zero. A
weighted step result is computed from its supplied path and costs, checks
length, finite non-negative values and NUMERIC overflow before emitting rows,
and never consults later source data. For a vertex mapping keyed by
`(tenant_id bigint, member_id uuid)`, a caller may use:

```sql
SELECT cassie_graph.vertex_ref(
  'public.social', 'person',
  ARRAY[cassie_graph.key_part(7::bigint),
        cassie_graph.key_part('00000000-0000-0000-0000-000000000001'::uuid)]
);
```

This is a **required building block**, not current functionality. `SqlArray`
can hold `Value` elements and record framing exists, but
`src/types/array/elements.rs` currently rejects composite arrays; removing that
guard alone is insufficient. Add and qualify composite arrays, nested
record/array codecs, typed SQL accessors and catalog discovery before graph
execution depends on them. Prove text and binary prepared result/input
roundtrips using real PostgreSQL drivers, including heterogeneous multi-column
keys, empty paths, escaped text, UUIDs, NULL distinctions, malformed input,
restart, resource accounting and stale references. If a needed published Midge
or client capability is unavailable, record the exact blocker rather than
substitute an opaque string or claim wire support.

A separate named-argument weighted path function is the selected extension.
Its selected SQL shape is:

```sql
graph_weighted_paths(
  graph text, source cassie_graph.graph_element_ref,
  target cassie_graph.graph_element_ref,
  weight_property text, max_hops integer, direction text DEFAULT 'out',
  edge_labels text[] DEFAULT NULL, max_cost numeric DEFAULT NULL,
  k integer DEFAULT 1
) RETURNS TABLE (path_rank bigint, cost numeric, path cassie_graph.graph_path,
                 edge_costs numeric[])
```

`graph` resolves a schema-qualified graph name. References must be vertices in
that graph; #1268 supplies a typed constructor and path projections. `max_hops`
is required and non-negative; `k` is positive; direction is `out`, `in`, or
`both`. NULL labels mean all edge labels and an empty array selects none.
NULL `max_cost` means no limit; otherwise it is finite, non-negative and
inclusive. Required NULL arguments return `22004`; invalid bounds or direction
return `22023`. Unknown properties return `42703` and incompatible declared
types return `42804`. Named and positional calls support defaults; duplicate or
unknown argument names reject. The weight property must exist with a compatible
exact numeric or integer type on each selected mapping; FLOAT weights are
rejected until their conversion and ordering semantics are selected and proven.
Accumulate as NUMERIC without silent rounding; an unrepresentable intermediate
or total returns `22003` and publishes no partial result. `edge_costs` retains
one validated weight per traversed edge in path order; its length must match
the path and its sum must equal `cost`. A zero-hop path has an empty array and
cost zero. Weighted step expansion consumes this sequence and never re-reads
weights from a later snapshot. Validate the sequence and account for it before
publication; malformed or incomplete results fail without a partial result.

Before returning any result, validate all graph-visible edges
in the selected edge mappings in the query snapshot for NULL, negative,
non-numeric and non-finite weights, unless an independently proven,
transactionally maintained invariant provides equivalent assurance. Invalid
weights fail deterministically; traversal order, predicate pushdown, `LIMIT`,
and cost bounds must not conceal them. A full controlled scan is the correct
fallback and its cost must be measured; O(degree) is not claimed for that path.
For example, `A -> T = 5`, `A -> B = 10`, `B -> T = -100` must error,
even when an early-stop traversal could return the first path. The start to
itself zero-hop path has cost zero. `k` counts paths,
with equal-cost paths broken by the same stable identity order. The function
returns SIMPLE paths; mixed edge types retain their mapping identity. Select an
algorithm only after semantics and
correctness oracles are fixed; a single settled-node set is not automatically
valid for k paths or hop-bounded alternatives. For the zero-cost cycle
`A -> B = 0`, `B -> A = 0`, source and target A, `max_hops = 2`, `k = 2`,
return both the zero-hop path and SIMPLE closing cycle, zero-hop first under the
prefix tie rule. This is an oracle requirement, not current implementation.

## Access, recovery and resources

The source tables remain authoritative. Derived adjacency is transactionally
maintained and is never allowed to silently omit or add an edge. The fast-path
design must state its integrity assumptions, proof, atomicity, snapshot and
recovery behavior against the published Midge API. The existing whole-graph
verifier protects a demonstrated missing-member case; a generation or count
alone is not evidence of completeness. Correlated damage to adjacency and its
certificate is a required case under any claimed corruption model. If fast
admission cannot prove a correct answer, use an exact controlled fallback or fail explicitly. Prove missing,
substituted, extra and stale members, interrupted updates, rollback and restart.

An exact local completeness proof is impossible under arbitrary correlated loss
of all derived entries and certificates without an independent trusted
authority: two snapshots can expose identical local derived bytes while one has
an additional unvisited authoritative incident edge. The current published
Midge transaction API has not been shown to supply authenticated predicate
completeness. A practical intermediate candidate verifies once per pinned
statement snapshot and reuses the result across hops, reducing repeated scans
without claiming cold O(degree) reads. It requires proof that every read uses
the same Data transaction and own-write overlay; current graph scans open their
own transactions. Missing proof leaves the existing exact verifier in place.

Resource accounting covers frontier, visited and retained path state, edge
decoding and output. Exhausting the query budget fails with `54000`; cancellation
and deadline handling must stop work and release reservations. A failed query
does not return a successful partial graph result. Read-count tests vary total
graph size independently of selected degree, and characterize high fan-out and
multi-hop work. The fast-path guarantee is conditional on an actually verified
complete sidecar, not a claim that damaged-state fallback is also O(degree).

## Compatibility, delivery and exit

Existing `CREATE GRAPH` and positional graph functions are Experimental. Provide
an explicit, documented conversion preserving old source tables and rows while
creating a mapped graph. If conversion cannot safely run, preserve the data and
give an actionable diagnostic. Specify the compatibility window and conversion
mechanics before removal; test restart and interruption at cutover. Graph writes
remain ordinary SQL table writes.

Implement in dependency order: contract and roadmap; completeness proof and
format, including an explicit blocker if the API cannot supply the required
proof; mapping and transaction visibility; native path/type building blocks;
parser/binder/executor vertical
slice; weighted paths; safe batching/pushdown; qualification and guide. Earlier
roadmap prerequisites remain gates. Stable requires actual client/wire,
independent semantic oracle, resource, corruption, lifecycle and benchmark
evidence on the release candidate, plus normal complete local and hosted gates.
Experimental support is not automatically promoted when code lands.

### Execution sequence

| Step | Owner | Exit before the next dependent step |
| --- | --- | --- |
| Contract | #1252 | Commit this contract, grammar and compatibility records; record the explicit completeness proof gap without claiming it solved. |
| Storage design | #1253 | Document Midge API evidence and a sound format/proof, or the exact blocker; preserve exact verification until proof and regressions pass. The intermediate once-per-snapshot option cannot close the O(degree) criterion. |
| Mapped graph | #1255 | Define, backfill and migrate graphs safely, with row/column authority, multi-graph fan-out, permissions and lifecycle evidence. |
| Transaction view | #1254 | Prove the same snapshot and overlay for all mapped tables, own writes, savepoints, cross-session writes and fallback. |
| Native path building block | #1268 | Implement types, composite arrays, catalog/wire codecs, constructors and real-client roundtrips. |
| Query surface and execution | #1256, #1257 | Bind and run the finite grammar with independent path oracles and cancellation/resource tests. Do not use pruning that changes multiplicity or path history. |
| Weighted paths | #1258 | Validate all applicable weights or prove a stronger invariant, then compare k paths and costs with exhaustive oracles. |
| Optimization | #1259 | Prove pushdown, batching and LIMIT preserve ordering, errors, NULLs, multiplicity and correlated inputs before enabling them. |
| Qualification | #1260 | Complete the guide, clients, lifecycle/corruption cases, measured scaling and exact-head local and hosted gates; decide promotion based on evidence. |

The GitHub native blockers carry the issue dependency graph. Independent design
preparation may proceed in parallel, but runtime implementation must respect
the proof and contract gates. No row in this table asserts a completed feature.

Excluded from this milestone: arbitrary multi-pattern joins inside one MATCH,
pattern unions, graph mutation syntax, graph analytics such as PageRank,
recursive-CTE optimization, distributed execution, and PostgreSQL extension
ABI compatibility. Unsupported forms must report a deterministic error.

## Decision evidence and remaining gates

The 2026-11-14 Jev review selected mapped graph pattern SQL (1.00), flagged
unbounded-WALK termination (0.91), correlated certificate damage (0.94), and
node-only settling with a hop bound (0.80). A follow-up prompted by the user's
instruction to include necessary building blocks selected native paths plus
ordered relational steps (1.00); heterogeneous identity risk scored 0.90.
The first review's 0.39 probability of a missing essential capability triggered
that follow-up, which identified path values (0.87). A further decision review
favored excluding dangling edges until endpoints exist (0.88), failing on invalid
weights (0.75), and typed paths with a lossless JSONB export (0.97). Its SIMPLE
weighted-path preference was weak (0.45 versus 0.23 WALK and 0.20 ACYCLIC),
so the selected SIMPLE rule requires the documented oracle tests. Its 0.91
precision-risk probe is evidenced by BIGINT `9007199254740993`, which is not
exactly representable as an IEEE-754 double: the JSON export uses tagged decimal
strings for integer values. Requests and responses are
retained under ignored `target/graph-v060-review/`. These judgments are leads,
not evidence of correctness or standard conformance.

Before dependent implementation, finish the normative grammar comparison,
validate the selected total order, freeze path type OIDs and wire encoding,
complete weighted-function diagnostic details, and establish the storage integrity proof. The proof
must explicitly address correlated sidecar/certificate damage: if the published
Midge API cannot establish the existing correctness contract and the desired
read bound together, record the API blocker and retain exact verification;
neither Jev nor a checksum assumption authorizes weaker results. Issues
#1252–#1260 and #1268 record the selected scope and native dependencies; the
contract itself must be committed before dependent implementation.
