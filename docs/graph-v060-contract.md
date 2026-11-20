# v0.6 graph contract and delivery plan

Status: **implemented finite Cassie contract in a pre-release project**. Owner decisions of 2026-11-15 define the selected surface; the
[qualification record](graph-v060-qualification.md) records executed correctness,
client, read-bound and benchmark evidence and remaining qualification limits. This
document does not claim SQL:2023 conformance. The source tables and ordinary
transactional indexes are authoritative; no derived adjacency proof is claimed.

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

`CREATE PROPERTY GRAPH` maps **existing** tables; creating or
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

The 2026-11-17 source-backed Jev decision selects a finite view lifecycle:
rename returns `2BP01` while a view in the graph's database depends on that
graph. The rejected statement preserves the old graph and its views; rename
without such dependents remains supported. A view in another database cannot
block graph drop or rename. Names retain ordinary quoted identifier identity.
Stored qualified references protect their precise same-database targets. Because
view SQL retains unqualified names without a captured creation search path, an
unqualified graph name potentially depends on every matching local graph identity
in that database; drop and rename protect each potential target.

The lifecycle guard covers every column a mapping reads: key, endpoint and
exposed property columns reject `DROP COLUMN` and type change with `2BP01`, a
key column also rejects `DROP NOT NULL`, and a constraint or index may be
dropped only while another non-partial unique key still covers exactly the
mapped key tuple. `DROP TABLE ... CASCADE`, `DROP COLUMN ... CASCADE` and a
`DROP SCHEMA ... CASCADE` that would orphan a graph of another schema return
`0A000`; a schema cascade drops the graphs the schema itself owns and the
same-database direct and transitive views that depend on those graphs, including
views in other schemas. A qualified graph or transitive view reference stays
precise. An ambiguous unqualified graph or relation reference refuses schema
cascade with `0A000` before any graph, view or schema is
mutated. Unique closures still cascade; unrelated views survive. View execution,
stored SQL and formats retain their existing behavior. The schema, graphs and view
closure are restored together on rollback. Graph DDL is
transactional and, like other DDL, is reserved to administrators (`42501`).
Dependency collection follows the binder's current lexical CTE lookup: each
nonrecursive body sees prior definitions, the main query sees all local
definitions, and a recursive arm sees itself. Qualified relation references
remain catalog dependencies; real dependencies inside CTE bodies remain visible.

Discovery uses the SQL/PGQ `information_schema` views `property_graphs`,
`pg_element_tables`, `pg_element_table_key_columns`,
`pg_edge_table_components`, `pg_element_table_labels`,
`pg_element_table_properties` and `pg_label_properties`, plus
`pg_catalog.pg_property_graph_identities`, which carries the durable database,
graph and mapping identities the standard views cannot. The administrative REST
catalog lists graphs in a `property_graphs` section. Every one of these lists a
graph only to a session that may read all of its mapped tables.

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
must publish its definition only after an initial consistent backfill succeeds,
once derived adjacency exists. Until the fast-reads step there is no derived
state to backfill and a definition publishes with its catalog transaction.
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
graph_table = GRAPH_TABLE ( graph_name MATCH [path_variable =] path [, path]
                           [KEEP selector] [WHERE sql_predicate]
                           COLUMNS (projection {, projection}) ) [AS alias]
path        = vertex {edge [quantifier] vertex}
vertex      = ( [variable] [IS label {| label}] [WHERE sql_predicate] )
edge        = - '[' [variable] [IS label {| label}] [WHERE sql_predicate] ']' ->
            | <- '[' [variable] [IS label {| label}] [WHERE sql_predicate] ']' -
            | - '[' [variable] [IS label {| label}] [WHERE sql_predicate] ']' -
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
are outside this initial grammar. The query examples describe the implemented finite surface:

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
distinct from element variables and binds a `cassie_graph.graph_path` value;
projecting it reports that named type in PostgreSQL Describe and result metadata.
For example:

```sql
SELECT g.path
FROM GRAPH_TABLE (
  social MATCH p = (a IS person WHERE a.id = $1) -[e IS knows]->{0,2} (b IS person)
  COLUMNS (p AS path)
) AS g;
```

The projected zero-hop value contains its one vertex and empty edge and
direction arrays. Qualify nonempty and zero-hop paths through prepared
statements, Describe, and real driver text and binary results. Labels select mappings; properties exposed by
the selected mappings must bind to one compatible SQL type or binding fails.
Bound values are parameters, not SQL text interpolation. One matching path
yields one row; parallel edges and distinct matching paths retain multiplicity.
No result order is promised without `ORDER BY`, except the documented choice
among tied paths when a selector explicitly limits the result.

A label alternative selects mappings as a set: a mapping carrying both `A` and
`B` contributes one candidate to `IS A|B`, not duplicate path matches.

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
self-loop is one matching edge step, not two duplicate orientations; its
canonical stored direction is source-to-destination (value 1), including in
path output and tie ordering. A
graph-pattern `WHERE` applies to selected complete paths, after a `KEEP`
selector. To constrain the search itself, place an edge predicate inline. An
outer SQL `WHERE` is also applied to rows after `GRAPH_TABLE` produces them.
For selective anchors, use an inline vertex predicate so it constrains the
search before selection. A planner may push an outer predicate into traversal
only after proving equivalence for selection, ties, NULLs, multiplicity,
errors and resource controls.

Recognized but unsupported graph constructs, including a third comma-separated
pattern, patterns that share no vertex variable, pattern unions, graph mutation, direct scalar use of group variables,
and unsupported quantifiers, return SQLSTATE `0A000`; malformed syntax returns
`42601`. Invalid bounds or a zero `k` return `22023`. Unknown variables or
properties and incompatible types use Cassie's corresponding existing binder
errors. Errors must not silently select a different meaning.

### Patterns joined on a shared vertex (#1353)

A `MATCH` may list two comma-separated paths. The list is the finite form of
correlated matching in this contract; it is not a general graph-pattern join.

* **Grammar.** `path , path`, after the optional path variable of the first path and
  before `KEEP`/`WHERE`/`COLUMNS`. At most two paths (`MAX_GRAPH_PATTERNS`); a third is
  `0A000`. A path variable (`p = ...`) with a list, on either path, is `0A000`. A leading
  or dangling comma is `42601`.
* **Shared variable identity.** A vertex variable named by both paths binds one vertex
  identity: the same vertex mapping and the same typed key tuple. Equality never compares
  properties or printed key components, so two tenants whose rows print the same ids stay
  separate. Naming the variable twice inside one path keeps its existing meaning.
  Only vertex variables join paths: an edge variable named by both paths, and any
  group variable, is `0A000`. Every path after the first must name a vertex variable that
  an earlier path names, or the statement is `0A000`: there is no cross product.
* **Label compatibility.** The labels named for a shared variable at each occurrence all
  apply. Some vertex mapping must carry every one of them; otherwise binding fails with
  `42804`. The variable exposes the properties of every label named at any occurrence, and
  a property resolves to one type exactly as for a repeated variable inside one path.
* **Scope.** `$n` parameters and the outer columns of a `LATERAL` source are visible in every
  inline predicate, the graph-pattern `WHERE` and `COLUMNS`, and parameter types are
  inferred from a comparison in any of them. An inline predicate sees only the variables of
  its own path (an unknown one is `42703`); put a condition across paths in the
  graph-pattern `WHERE`, which sees every variable of both.
* **Selectors and modes.** `KEEP` applies to each path independently, before the join, and
  the graph-pattern `WHERE` applies to the joined match. Unbounded `WALK` is rejected in
  either path as for a single path.
* **Bag semantics.** Every combination of one selected path from each pattern whose shared
  vertices are equal is one row. Parallel edges and distinct paths multiply, so a
  vertex reached by two parallel edges from each side yields four rows. `DISTINCT` and
  aggregation belong to the enclosing `SELECT`.
* **Bounds.** Each path keeps its quantifier bounds. A work or memory bound is the existing
  query memory budget: frontier, visited, retained path and joined-tuple state are
  accounted, exceeding it is `54000`, and cancellation or a deadline is `57014`. Either
  failure returns no row of the statement, never a partial match set.
* **Execution.** Each path is searched on its own over one statement store using the
  existing indexed exact reads, and the selected paths are joined in memory on the shared
  vertex identities. No sidecar or derived index exists. When a later path starts at a
  shared variable, its search starts only at the vertices the earlier paths bound to it;
  otherwise it uses its own start lookup under the same read rules.
* **Order.** The implementation emits joined rows in lexicographic order of each path's
  position in that path's documented total order. As elsewhere, only `ORDER BY` promises
  an order.
* **Evidence.** `EXPLAIN` reports `patterns=2 join=shared_vertex` and the access path of each
  `pattern[i]`. The runtime metrics `read_paths.graph_last_branch_expansions` and
  `graph_last_branch_paths` hold, for the latest joined read, the edge hops taken and the
  paths selected by each pattern before the join.

Binding selects these readings where the grammar is silent. A projection that
is a bare `variable.property` is named after the property; any other projection
needs `AS column_alias` (`42601` without one), and duplicate output names are
`42701`. Inside a `LATERAL` `GRAPH_TABLE`, a pattern variable shadows an outer
relation of the same name. A subquery inside a graph predicate or projection,
`WITH ORDINALITY`, and repeating a quantified edge variable are `0A000`. An
unknown graph or label is `42704`. Reading a graph requires `SELECT` on every
table the pattern can read, including the endpoint vertex tables of each
selected edge mapping. A view over `GRAPH_TABLE` depends on its graph:
`DROP PROPERTY GRAPH` returns `2BP01` while such a view exists.

Execution selects these readings. Two adjacent variable-length steps can reach
one path through different hop splits: the splits are one match and one row
unless they bind a named singleton variable to different elements, in which
case each binding is its own match, whether or not the variable is projected.
An inline predicate that reads only its own element is evaluated once per
element; one that reads another pattern variable is evaluated on each complete
candidate path, still before any selector, which yields the same matches. When
several labels named by an element expose one property from different columns
of a mapping, the first named label the mapping carries supplies the value.
A graph-bearing source is computed completely before its first row is
returned and is never streamed, so a failing evaluation emits no row.

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
  the interleaved sequence `V0, E0, direction0, V1, E1, direction1, V2, ...`:
  element references order by
  durable database ID, graph ID, kind, mapping ID and key tuple; key components
  order first by declared type (smallint, integer, bigint, text, uuid), then by
  that type's value (C byte order for text and unsigned 16-byte order for UUID).
  Compare direction after each edge identity, forward before reverse, and the
  shorter sequence first when one is a prefix. These identity IDs and ordering
  must survive restart. For equal-cost A→B→D with edges z1,z2 and A→C→D
  with edges a1,a2, if a1 precedes z1 the path through C sorts first even if B
  precedes C; an empty zero-hop path sorts before its closing-cycle extension.
  Do not prune solely by visited node
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
  it. An undirected self-loop produces one step with direction 1 on repeated
  runs and restart, and parallel edges produce
  distinct matches. Quantified `{1,1}` still binds a group variable.
* An undirected non-loop edge stored from A to B records direction 1 for A to B
  and 2 for B to A. A mapping labeled both `A` and `B` is included once, not
  twice, by a single `IS A|B` pattern.
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

The 2026-11-17 source-backed Jev decision uses structural SQL array keys for
array fields of all native composite values, including graph references. Compare
elements recursively with NULL after non-NULL, then element count, dimension
rank, all dimension lengths, and finally all lower bounds. Integer components
retain exact signed integer comparison. SQL comparison keys do not stringify
native record arrays; the internal graph path tie order remains the order
specified by this contract.

Database, graph and mapping IDs are durable UUID logical identities, rather
than SQL names or type OIDs: rename preserves them; a dropped and recreated object
gets a new identity. An operation resolving a reference from a different
database or a removed graph/mapping fails deterministically instead of
retargeting it. Element identity is mapping plus typed key, with no row
incarnation: after delete and reinsertion of the same key in the same mapping,
the old reference resolves to the replacement row in a later snapshot.
The selected and implemented image policy (#1327, closing the image portion of
#1268) is clone-only: restore always creates a new logical database with a fresh
database UUID. The image's stored
database identity and property graph definitions are not copied; the clone
allocates its own database identity on first use, and graphs are recreated in
it by `CREATE PROPERTY GRAPH` with fresh graph and mapping identities. Copied
graph-reference values retain their source identities and resolve as `42704` in
the clone; detached structural inspection remains possible. There is no remap
operation or identity-preserving restore into an existing logical database.
A reference never silently retargets across databases. Database, graph and
mapping identities use the durable catalog allocators. Public labels and
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
custom decoder. `path_to_jsonb` returns version 1 as a JSONB object with exactly
`version` (JSON integer 1), `vertices`, `edges`, and `directions` fields. Vertex
and edge arrays preserve path order; directions contains the numeric values 1 or
2. Every element object has `database_id`, `graph_id`, `mapping_id` (lowercase
canonical UUID strings), `kind` (`vertex` or `edge`), and `key_parts`. Each
ordered key part has exactly `type` (`smallint`, `integer`, `bigint`, `text`,
or `uuid`) and `value` (a JSON string). Integer strings use canonical base-10
without leading zeros except `0`; UUID strings are lowercase canonical form;
text is the exact SQL text value represented as a JSON string. JSON escaping
and JSONB object field order are not identity. No numeric key is a JSON number,
so consumers cannot round BIGINT values. No field is omitted or JSON null.
For example, a zero-hop path may export:

```json
{"version":1,"vertices":[{"database_id":"00000000-0000-0000-0000-000000000001","graph_id":"00000000-0000-0000-0000-000000000002","mapping_id":"00000000-0000-0000-0000-000000000003","kind":"vertex","key_parts":[{"type":"bigint","value":"9007199254740993"}]}],"edges":[],"directions":[]}
```

A one-edge heterogeneous path can export:

```json
{"version":1,"vertices":[{"database_id":"00000000-0000-0000-0000-000000000001","graph_id":"00000000-0000-0000-0000-000000000002","mapping_id":"00000000-0000-0000-0000-000000000003","kind":"vertex","key_parts":[{"type":"bigint","value":"7"}]},{"database_id":"00000000-0000-0000-0000-000000000001","graph_id":"00000000-0000-0000-0000-000000000002","mapping_id":"00000000-0000-0000-0000-000000000004","kind":"vertex","key_parts":[{"type":"uuid","value":"00000000-0000-0000-0000-000000000005"}]}],"edges":[{"database_id":"00000000-0000-0000-0000-000000000001","graph_id":"00000000-0000-0000-0000-000000000002","mapping_id":"00000000-0000-0000-0000-000000000006","kind":"edge","key_parts":[{"type":"text","value":"a\"b"}]}],"directions":[1]}
```

JSON is an export, not the native identity or equality mechanism.

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
partially visible. Use the persistent schema-family `Midge::allocate_object_id`
precedent for named composite type, array and `relkind = 'c'` relation OIDs;
the allocator has no database argument, while named type metadata is scoped to
the database. Do not use
OIDs as the durable graph identities above. Type and array OIDs and their fields
must be discoverable through `pg_type`, `pg_class` and `pg_attribute`. They
remain stable across restart for the life of the database, but clients must
discover them rather than assume fixed or globally shared OIDs. Restore and
clone must implement and test the identity policy above; an old reference must
never silently resolve to a different mapping.
Use PostgreSQL composite and array text and binary framing, including nested
quoting, element type OIDs, NULLs and array bounds. Composite binary fields use
big-endian signed 32-bit field count followed by each field's 32-bit type OID,
signed 32-bit byte length (`-1` for NULL), and native type payload. Array binary
uses big-endian signed 32-bit dimension count and null flag, 32-bit element OID,
each dimension's signed 32-bit length and lower bound, then signed 32-bit
length-prefixed native element payloads (`-1` for NULL); path invariants reject
NULL elements and noncanonical bounds. Empty arrays use zero dimensions.
Text fields and nested arrays use PostgreSQL's composite and array text quoting
and escaping. Exact OIDs are allocated and persisted, not globally preassigned.
Prepared statements and
result metadata must preserve the same types. Some drivers require explicit
registration to decode named composite arrays; the scalar expansion interface
must work without it. PostgreSQL composite types cannot themselves enforce the
tagged-part invariant: constructors and text/binary input paths must validate
it, and every operation validates shape and kind regardless of construction
path. A constructed path is a detached structural value and may represent
historical data; its existence does not attest that its edges connect its
vertices in the current graph snapshot. A query-produced path is connected in
the snapshot in which it was produced. Structural equality and step expansion
inspect the retained value without requiring current rows. Operations that use
a path to access the graph must additionally resolve its references, check
current-snapshot connectivity and direction and enforce authorization before
use; malformed, disconnected or unauthorized input fails rather than silently
reading a different path. Malformed text or binary input returns `22P02` or `22P03`, respectively;
well-formed composites with invalid tags, slots, array shapes or inconsistent
path lengths or disconnected input returns `22023`. Graph resolution of a stale
or cross-database reference returns `42704`, and insufficient authority returns
`42501`. These checks apply to
values produced by SQL `ROW` casts or subsequent array/field mutation as well
as direct parameters. Tests cover all construction paths and errors.

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
mapping alias in the requested element kind. Use the exact stored alias
spelling supplied as `mapping` TEXT: `n` and `N` remain distinct after DDL
identifier folding. Quotation marks inside that TEXT do not introduce a second
identifier grammar. Constructors resolve the typed
tuple in that mapping's current transaction view, check key arity and exact
SQL types and require the element to exist and be readable. NULL required
arguments return `22004`; wrong type or arity returns `42804`; an unknown graph,
mapping or element returns `42704`; insufficient read authority returns `42501`.
Authorization must not disclose protected element properties.
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
`both`. NULL labels mean all edge labels and an empty array selects none;
NULL elements inside a non-NULL labels array return `22004`. Explicit NULL
`direction` or `k` returns `22004`; omitting them uses their defaults.
NULL `max_cost` means no limit; otherwise it is finite, non-negative and
inclusive. Required NULL arguments return `22004`; invalid bounds or direction
return `22023`. A well-formed edge reference supplied as source or target
returns `22023`; an unknown graph, mapping, label or missing element and a
cross-graph or cross-database reference return `42704`; insufficient authority
returns `42501`. Unknown properties return `42703` and incompatible declared
types return `42804`. Named and positional calls support defaults; duplicate
argument names or positional arguments after named ones return `42601`.
Unknown argument names that do not resolve an overload return `42883`. The weight property must exist with a compatible
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
weights fail with `22023`; traversal order, predicate pushdown, `LIMIT`,
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

The source tables remain authoritative. Graph reads are exact reads of them in the
statement's snapshot and overlay. The fast path (#1253, #1254, #1259) does not add a
derived adjacency sidecar, certificate or verifier: it expands lazily from the frontier
over the transactional indexes the product already maintains, and its design, integrity
model, fallback rules and read bounds are specified in `docs/performance-contracts.md`
("Graph Reads").

- A vertex is fetched by the unique key that backs its mapping.
- Incident edges are fetched through an ordinary scalar index on the edge table's endpoint
  columns when one exists. Leading endpoint columns also qualify when every trailing
  index column is provably `NOT NULL`; nullable suffixes require the exact fallback
  because scalar indexes can omit those rows. An edge mapping without a usable index
  on a needed endpoint side
  falls back to the exact full read of that mapping for the statement; a partial
  neighborhood is never used.
- Start vertices use point lookups only when the start pattern's inline predicate fixes the
  complete key by equality with constants or parameters; otherwise the start mapping is read
  exactly.
- Integrity is the integrity of every indexed SQL query: rows, scalar index entries and
  unique reservations are key prefixes of one Midge column family and commit atomically
  together, and index reads happen in the statement's snapshot with the transaction overlay
  merged. Arbitrary storage corruption of an index is out of the model, exactly as for
  indexed SQL queries; graph reads claim no stronger detection.
- `graph_weighted_paths` keeps its full controlled validation scan of the selected edges and
  is O(selected edges) by contract (#1355): invalid-weight rejection must hold for every
  graph-visible edge, including edges outside the visited neighbourhood, and no sidecar may
  certify weights. The scan decodes only each edge row's key, endpoint and weight columns,
  never its other properties, and is counted separately as `graph_validation_rows`.
- The `cassie_graph.vertex_ref` and `edge_ref` constructors find the element through the
  scalar index that backs the mapped key (one index entry and one row read, plus the
  transaction's own pending changes, plus the two endpoint vertex lookups for an edge); only
  when no usable index exists do they scan the mapped table. They never scan it otherwise.
- Counted bounds (#1355), all in `read_paths.graph_*` and `EXPLAIN ANALYZE graph_reads=`:
  degree `d` costs `1 + 2d` index entries and row fetches, `d` edges admitted and one
  neighbourhood expansion; depth `n` over a path costs `n` expansions and `n` admitted
  edges; unreachable edges cost nothing; a hub is paid for once when the traversal reaches
  it and not at all when it is not reached. Evidence: `tests/graph_table/cost_curves.rs`
  and `tests/graph_weighted/cost_curves.rs`. Timing criteria stay open: the machine is never
  quiet, so no p95 is recorded or claimed.
- Read counts are tested by varying total graph size independently of selected degree and by
  characterizing fallback separately. The O(degree) claim is conditional on the endpoint
  index existing and on an anchored start; fallback and unanchored starts are stated
  separately.

Resource accounting covers frontier, visited and retained path state, edge
decoding and output. Exhausting the query budget fails with `54000`; cancellation
and deadline handling must stop work and release reservations. To guarantee no
successful partial graph result across suspended pgwire portals, finish and
materialize the complete graph-bearing statement result and its controlled
output encoding before emitting the first successful page; an evaluation or
encoding error emits no page, and retention is accounted against the query
budget. Later Execute calls page the already completed result, respecting
cumulative portal limits. Cancellation or transport failure during later
delivery can leave previously delivered pages visible; Cassie cannot retract
bytes already sent, and the failed Execute emits no successful page. The
2026-11-17 source-backed Jev decision selects retained preflight: admit reusable
largest-frame storage against the materialized portal owner's tracker, encode
every row with final formats and global row indices, and validate description
and completion frames before first delivery. Retain that storage and its lease
through suspension and reuse it for later pages. Qualification evidence belongs
in the qualification record; this contract alone does not establish a pass.
Read-count tests vary total
graph size independently of selected degree, and characterize high fan-out and
multi-hop work.

## Compatibility, delivery and exit

**Owner decision, 2026-11-15: the legacy graph feature is removed without
conversion.** `CREATE GRAPH`, its fixed `g_nodes` and `g_edges` tables and the
positional `graph_neighbors`, `graph_expand` and `graph_shortest_path` functions
have no production usage; Cassie is pre-release and makes
no on-disk compatibility promise. They are deleted once mapped property graphs,
`GRAPH_TABLE` and `graph_weighted_paths` cover their uses, and their tests and
benchmark are migrated to mapped graphs. No conversion tool, equivalence
preflight or compatibility window is provided. A store that still holds a
legacy graph definition after the removal ignores it and deletes the stored
legacy definitions and adjacency entries at startup (implemented by #1258); its
former node and edge tables remain ordinary tables that a new graph may map.
Graph writes remain ordinary SQL table writes.

The removal deliberately drops three legacy behaviours that the mapped model
does not carry: traversal over edges whose endpoint rows do not exist, FLOAT
edge weights, and per-row node and edge type values in place of mapping labels.
A table that relied on them must add the missing vertex rows, use an exact
numeric weight column and declare labels on mappings before it is mapped. Jev
rated the chance that removal leaves a capability gap at 0.37; these three are
the known gap and are accepted.

**Owner decision, 2026-11-15: features first on exact reads.** v0.6.0 query
execution reads the mapped vertex and edge tables exactly, inside the
statement's own Data snapshot and transaction overlay. That is correct by
construction with the published Midge API and needs no derived state. Derived
adjacency, its completeness argument and the O(degree) read bound are an
optimization delivered last; if the bound cannot be justified against the
published API, the blocker is recorded and exact reads remain. Feature qualification requires actual client and wire, independent semantic oracle, resource,
lifecycle and benchmark evidence on the release candidate, plus the normal
complete local and hosted gates. Implementation alone does not establish
those evidence obligations.

### Execution sequence

| Step | Owner | Exit before the next dependent step |
| --- | --- | --- |
| Contract | #1252 | This contract, including the two owner decisions above. |
| Mapped graph | #1255 | Define, drop and rename graphs over existing tables with durable identities, key and endpoint validation, lifecycle guards, permissions and discovery. No derived state is built. |
| Native path building block | #1268 | Composite arrays, the reserved types, catalog and wire codecs, constructors and real-client round trips. |
| Query surface | #1256 | Parse and bind the finite grammar with typed Describe output and deterministic rejections. |
| Exact execution | #1257, #1254 (transaction view) | Run the grammar on exact reads with independent path oracles, own-write and savepoint visibility, cancellation and resource tests. Do not use pruning that changes multiplicity or path history. |
| Weighted paths and legacy removal | #1258 | Validate all applicable weights, compare k paths and costs with exhaustive oracles, then delete the legacy feature and migrate its tests. |
| Fast reads | #1253, #1254 (read bounds), #1259 | Record the integrity model and completeness argument or the exact blocker; add adjacency, per-anchor overlay merge, batching and pushdown only with results identical to exact execution. |
| Qualification | #1260 | Guide, clients, lifecycle cases, measured scaling and exact-head local and hosted gates; record qualification results and remaining limits. |

The GitHub native blockers carry the issue dependency graph; where they
disagree with this table, this table is the delivery order. Independent design
preparation may proceed in parallel. No row in this table asserts a completed feature.

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

Before dependent implementation, qualify the selected total order, implement and
qualify the selected type/OID and wire contracts, and establish the storage
integrity proof. Compare the normative grammar before any
future standard-conformance claim, not as a prerequisite to implementing the
selected Cassie dialect. The proof
must explicitly address correlated sidecar/certificate damage: if the published
Midge API cannot establish the existing correctness contract and the desired
read bound together, record the API blocker and retain exact verification;
neither Jev nor a checksum assumption authorizes weaker results. Issues
#1252–#1260 and #1268 record the selected scope and native dependencies; the
contract itself must be committed before dependent implementation.
