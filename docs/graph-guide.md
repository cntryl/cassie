# Property graph guide

This guide explains how to query the relationships stored in your ordinary Cassie
tables with SQL property graphs. It covers defining a graph, querying it with
`GRAPH_TABLE`, using paths as values, finding weighted routes, and the rules that apply
to transactions, permissions and schema changes. The selected grammar and semantics are
specified in the [v0.6 graph contract](graph-v060-contract.md); this guide describes what
the current build does and states its limits.

Every SQL example in this guide is executed by the integration suite
[`tests/graph_guide.rs`](../tests/graph_guide.rs) against an in-memory Cassie instance, and
each documented result or SQLSTATE is compared with the observed one. The guide file itself
is the only copy of the statements, so an example cannot change without the suite noticing.
Examples that share a "world" run in document order against one fresh instance, so a later
example may rely on the tables and rows created by earlier ones. Result blocks show one row
per line with columns separated by ` | `.

The feature is Cassie's own dialect of SQL property graph queries. It is not a claim of
SQL:2023 (SQL/PGQ) conformance, and it is not a Cypher or graph-database interface. Feature
status is recorded in [Feature support](feature-support.md).

## 1. What a mapped property graph is

Many read models already store relationships: a `memberships` table that links users to
groups, a `follows` table, a `reports_to` table. A projector keeps those tables current from
an event stream. A **property graph** lets you ask path questions about those tables
(who can reach this document, who are the ancestors of this employee, what is the cheapest
route) without copying the data into another structure.

A property graph is a catalog object that **maps** existing tables:

- A **vertex table** supplies the vertices. Each row is one vertex, identified by a key.
- An **edge table** supplies the edges. Each row is one edge, with its own key and two
  endpoint keys that refer to vertices.
- A **label** names a mapping, so a pattern can say which kind of vertex or edge it means.
- **Properties** are the columns a label exposes to patterns.

Creating a graph copies no rows and creates no tables. Dropping a graph deletes no rows and
no tables. Graph reads always run against the current table rows in the statement's own
transaction view, so a write to a mapped table is visible to the next graph query that the
same transaction runs, and to other sessions when it commits. Cassie does not maintain a
separate adjacency structure in this release; every graph query reads the mapped tables.

A graph is a read-only view of tables. There is no syntax that inserts, updates or deletes
vertices or edges through a graph. Change the tables with ordinary SQL.

## 2. Defining a graph

<!-- world: social -->

The examples in this section and in the querying sections use two small tables: people, and
the follow relationships between them. Some follow edges are inactive, and one of them
(Ada follows Cy) has a more expensive but active detour through Ben.

<!-- example: social-create-people -->
```sql
CREATE TABLE people (id BIGINT PRIMARY KEY, name TEXT NOT NULL, team TEXT NOT NULL)
```

<!-- example: social-insert-people -->
```sql
INSERT INTO people VALUES (1, 'Ada', 'eng'), (2, 'Ben', 'eng'), (3, 'Cy', 'ops'), (4, 'Di', 'ops'), (5, 'Eli', 'eng')
```

<!-- example: social-create-follows -->
```sql
CREATE TABLE follows (id BIGINT PRIMARY KEY, follower_id BIGINT NOT NULL, followee_id BIGINT NOT NULL, active BOOLEAN NOT NULL, cost INT NOT NULL)
```

<!-- example: social-insert-follows -->
```sql
INSERT INTO follows VALUES (1, 1, 2, TRUE, 1), (2, 2, 3, TRUE, 1), (3, 1, 3, FALSE, 5), (4, 3, 4, TRUE, 2), (5, 4, 1, TRUE, 1), (6, 2, 5, TRUE, 3)
```

`CREATE PROPERTY GRAPH` names the graph, its vertex tables and its edge tables:

<!-- example: social-create-graph -->
```sql
CREATE PROPERTY GRAPH social
  VERTEX TABLES (
    people AS person KEY (id) LABEL person PROPERTIES (id, name, team)
  )
  EDGE TABLES (
    follows AS knows KEY (id)
      SOURCE KEY (follower_id) REFERENCES person (id)
      DESTINATION KEY (followee_id) REFERENCES person (id)
      LABEL knows PROPERTIES (active, cost)
  )
```

Each part of the definition has a fixed meaning:

- `people AS person` maps the table `people` under the **alias** `person`. The alias is
  required, and must be unique among the vertex mappings (and among the edge mappings) of
  the graph. A vertex mapping and an edge mapping may share an alias.
- `KEY (id)` declares the columns that identify an element. A key can have several columns.
- `SOURCE KEY (...) REFERENCES person (id)` and `DESTINATION KEY (...)` name the edge-table
  columns that hold the endpoint keys, and the vertex alias and key columns they refer to.
  The referenced columns must be exactly the ordered key of that vertex mapping, with the
  same number of columns and the same SQL types.
- `LABEL knows PROPERTIES (active, cost)` gives the mapping a label and lists the columns the
  label exposes. A property keeps its column name unless you write `column AS name`. A
  mapping must declare at least one label and may declare several. Patterns refer to labels,
  not to table names or aliases.

### Keys

A key identifies one element, so it must be backed by a unique key that Cassie enforces for
the whole table: a primary key, a `UNIQUE` constraint or a non-partial unique index on
exactly that ordered column list, with every key column declared `NOT NULL` (a primary key
implies it). Key columns can use these types: `SMALLINT`, `INTEGER`, `BIGINT`, `TEXT` and
`UUID`. Text keys compare as bytes (C collation). Floating-point, boolean, numeric,
domain and other types are rejected, as are views and materialized views as sources: map
the base table instead.

An element's identity is its mapping plus its typed key. Two tables may use the same key
values without being confused, and renaming a graph, table or column keeps identities.

### Rows that do not form an edge

Cassie adds no foreign key and no cascading delete to your tables. An edge takes part in a
graph query only when both of its endpoint vertices exist and its endpoint columns are not
null in the statement's snapshot. A row whose endpoint is missing remains an ordinary row
and becomes visible as an edge once the missing vertex is inserted
([section 7](#7-transactions-and-visibility) shows this).

### Validation errors

`CREATE PROPERTY GRAPH` validates the whole definition and publishes it atomically, or fails
and changes nothing. These are the errors you are most likely to meet. The examples use the
tables below and a composite-keyed graph over them.

<!-- world: define -->

<!-- example: define-create-accounts -->
```sql
CREATE TABLE accounts (tenant TEXT NOT NULL, id BIGINT NOT NULL, name TEXT, PRIMARY KEY (tenant, id))
```

<!-- example: define-create-transfers -->
```sql
CREATE TABLE transfers (id UUID PRIMARY KEY, from_tenant TEXT NOT NULL, from_id BIGINT NOT NULL, to_tenant TEXT NOT NULL, to_id BIGINT NOT NULL, amount NUMERIC(12,2), note TEXT)
```

A graph with a two-column key, a property renamed with `AS`, and an edge mapping that carries
two labels with different properties:

<!-- example: define-create-ledger -->
```sql
CREATE PROPERTY GRAPH ledger
  VERTEX TABLES (
    accounts AS account KEY (tenant, id) LABEL account PROPERTIES (tenant, id, name AS display_name)
  )
  EDGE TABLES (
    transfers AS transfer KEY (id)
      SOURCE KEY (from_tenant, from_id) REFERENCES account (tenant, id)
      DESTINATION KEY (to_tenant, to_id) REFERENCES account (tenant, id)
      LABEL transfer PROPERTIES (amount)
      LABEL audited PROPERTIES (amount, note)
  )
```

Creating a graph whose name exists fails with `42710`:

<!-- example: define-duplicate-graph -->
```sql
CREATE PROPERTY GRAPH ledger
  VERTEX TABLES (accounts AS account KEY (tenant, id) LABEL account PROPERTIES (id))
  EDGE TABLES (
    transfers AS transfer KEY (id)
      SOURCE KEY (from_tenant, from_id) REFERENCES account (tenant, id)
      DESTINATION KEY (to_tenant, to_id) REFERENCES account (tenant, id)
      LABEL transfer PROPERTIES (amount)
  )
```
```text sqlstate
42710
```

A missing table is `42P01` and a missing column, whether in a key, an endpoint or a
property list, is `42703`:

<!-- example: define-missing-table -->
```sql
CREATE PROPERTY GRAPH g1
  VERTEX TABLES (ghosts AS ghost KEY (id) LABEL ghost PROPERTIES (id))
  EDGE TABLES (
    transfers AS transfer KEY (id)
      SOURCE KEY (from_tenant, from_id) REFERENCES ghost (id)
      DESTINATION KEY (to_tenant, to_id) REFERENCES ghost (id)
      LABEL transfer PROPERTIES (amount)
  )
```
```text sqlstate
42P01
```

<!-- example: define-missing-column -->
```sql
CREATE PROPERTY GRAPH g2
  VERTEX TABLES (accounts AS account KEY (tenant, id) LABEL account PROPERTIES (tenant, id, colour))
  EDGE TABLES (
    transfers AS transfer KEY (id)
      SOURCE KEY (from_tenant, from_id) REFERENCES account (tenant, id)
      DESTINATION KEY (to_tenant, to_id) REFERENCES account (tenant, id)
      LABEL transfer PROPERTIES (amount)
  )
```
```text sqlstate
42703
```

A key without a matching enforced unique key, a nullable key column, or a key that is wider
than the unique key is `42830`:

<!-- example: define-create-loose -->
```sql
CREATE TABLE loose (id BIGINT, label TEXT)
```

<!-- example: define-key-not-unique -->
```sql
CREATE PROPERTY GRAPH g3
  VERTEX TABLES (loose AS item KEY (id) LABEL item PROPERTIES (id))
  EDGE TABLES (
    transfers AS transfer KEY (id)
      SOURCE KEY (from_tenant, from_id) REFERENCES item (id)
      DESTINATION KEY (to_tenant, to_id) REFERENCES item (id)
      LABEL transfer PROPERTIES (amount)
  )
```
```text sqlstate
42830
```

An endpoint whose columns do not match the referenced key in number or type is `42830` for
the wrong number of columns and `42804` for a type mismatch:

<!-- example: define-create-text-edges -->
```sql
CREATE TABLE text_edges (id BIGINT PRIMARY KEY, a TEXT NOT NULL, b TEXT NOT NULL)
```

<!-- example: define-create-nodes -->
```sql
CREATE TABLE nodes (id BIGINT PRIMARY KEY, label TEXT)
```

<!-- example: define-endpoint-type -->
```sql
CREATE PROPERTY GRAPH g4
  VERTEX TABLES (nodes AS node KEY (id) LABEL node PROPERTIES (id))
  EDGE TABLES (
    text_edges AS link KEY (id)
      SOURCE KEY (a) REFERENCES node (id)
      DESTINATION KEY (b) REFERENCES node (id)
      LABEL link PROPERTIES (id)
  )
```
```text sqlstate
42804
```

A key column type that cannot identify elements, such as `DOUBLE PRECISION`, and a source that
is not an ordinary table, such as a view, are `0A000`:

<!-- example: define-create-float-keys -->
```sql
CREATE TABLE float_nodes (id DOUBLE PRECISION PRIMARY KEY)
```

<!-- example: define-unsupported-key-type -->
```sql
CREATE PROPERTY GRAPH g5
  VERTEX TABLES (float_nodes AS node KEY (id) LABEL node PROPERTIES (id))
  EDGE TABLES (
    text_edges AS link KEY (id)
      SOURCE KEY (a) REFERENCES node (id)
      DESTINATION KEY (b) REFERENCES node (id)
      LABEL link PROPERTIES (id)
  )
```
```text sqlstate
0A000
```

<!-- example: define-create-node-view -->
```sql
CREATE VIEW node_view AS SELECT id, label FROM nodes
```

<!-- example: define-view-source -->
```sql
CREATE PROPERTY GRAPH g6
  VERTEX TABLES (node_view AS node KEY (id) LABEL node PROPERTIES (id))
  EDGE TABLES (
    text_edges AS link KEY (id)
      SOURCE KEY (a) REFERENCES node (id)
      DESTINATION KEY (b) REFERENCES node (id)
      LABEL link PROPERTIES (id)
  )
```
```text sqlstate
0A000
```

A repeated alias, label or property name inside one mapping is `42710`, and malformed
syntax is `42601`:

<!-- example: define-duplicate-property -->
```sql
CREATE PROPERTY GRAPH g7
  VERTEX TABLES (nodes AS node KEY (id) LABEL node PROPERTIES (id, id))
  EDGE TABLES (
    text_edges AS link KEY (id)
      SOURCE KEY (a) REFERENCES node (id)
      DESTINATION KEY (b) REFERENCES node (id)
      LABEL link PROPERTIES (id)
  )
```
```text sqlstate
42710
```

<!-- example: define-syntax-error -->
```sql
CREATE PROPERTY GRAPH g8 VERTEX TABLES ()
```
```text sqlstate
42601
```

A missing schema is `3F000`. Only administrators may run graph DDL; any other role gets
`42501` ([section 8](#8-permissions)). Every statement above fails without creating
anything, so you can correct the statement and run it again.

## 3. Querying with GRAPH_TABLE

<!-- world: social -->

`GRAPH_TABLE` is a table source. You use it in the `FROM` clause of an ordinary `SELECT`,
give it an alias, and name the graph, a `MATCH` pattern and the `COLUMNS` to return:

<!-- example: pattern-one-hop -->
```sql
SELECT t.friend
FROM GRAPH_TABLE (
  social MATCH (a IS person WHERE a.id = 1) -[IS knows]-> (b IS person)
  COLUMNS (b.name AS friend)
) AS t
ORDER BY t.friend
```
```text result
Ben
Cy
```

The pattern `(a IS person WHERE a.id = 1) -[IS knows]-> (b IS person)` has two vertex
patterns and one edge pattern between them. A vertex pattern is `(variable IS label WHERE
condition)`, and every part is optional. An edge pattern is `-[variable IS label WHERE
condition]->`. Variables bind the matched vertices and edges, and `COLUMNS` and the
predicates read their properties as `variable.property`. A projection that is exactly
`variable.property` is named after the property; any other projection needs `AS alias`.
`IS a | b` accepts either of two labels.

Each matching path produces one row. Parallel edges and distinct paths that reach the same
vertex produce separate rows, as the later examples show. No row order is promised, so use
`ORDER BY` when order matters.

### Directions

`->` follows an edge from its source to its destination, `<-` follows it backwards, and a
plain `-` accepts either direction:

<!-- example: pattern-reverse -->
```sql
SELECT t.follower
FROM GRAPH_TABLE (
  social MATCH (a IS person WHERE a.name = 'Cy') <-[IS knows]- (b IS person)
  COLUMNS (b.name AS follower)
) AS t
ORDER BY t.follower
```
```text result
Ada
Ben
```

<!-- example: pattern-either-direction -->
```sql
SELECT t.neighbour
FROM GRAPH_TABLE (
  social MATCH (a IS person WHERE a.name = 'Ada') -[IS knows]- (b IS person)
  COLUMNS (b.name AS neighbour)
) AS t
ORDER BY t.neighbour
```
```text result
Ben
Cy
Di
```

### Quantifiers

A quantifier after an edge pattern repeats the edge: `{n}` is exactly n edges, `{m,n}` is
between m and n inclusive, `+` is one or more and `*` is zero or more. The edge variable
becomes a *group*: it stands for the ordered list of repeated edges, so you can read it only
inside its own inline `WHERE` ([section 3.4](#34-predicates)). Vertices that lie between
repetitions are not constrained by the declared vertex patterns; only the first and the last
vertex are.

<!-- example: quantifier-range -->
```sql
SELECT t.reached
FROM GRAPH_TABLE (
  social MATCH (a IS person WHERE a.name = 'Ada') -[IS knows]->{1,2} (b IS person)
  COLUMNS (b.name AS reached)
) AS t
ORDER BY t.reached
```
```text result
Ben
Cy
Cy
Di
Eli
```

Cy appears twice because two different paths reach Cy within two edges. With `{2}`, only
paths of exactly two edges match:

<!-- example: quantifier-exact -->
```sql
SELECT t.reached
FROM GRAPH_TABLE (
  social MATCH (a IS person WHERE a.name = 'Ada') -[IS knows]->{2} (b IS person)
  COLUMNS (b.name AS reached)
) AS t
ORDER BY t.reached
```
```text result
Cy
Di
Eli
```

A lower bound of zero matches the start vertex itself, with no edge:

<!-- example: quantifier-zero -->
```sql
SELECT t.reached
FROM GRAPH_TABLE (
  social MATCH (a IS person WHERE a.name = 'Ada') -[IS knows]->{0,1} (b IS person)
  COLUMNS (b.name AS reached)
) AS t
ORDER BY t.reached
```
```text result
Ada
Ben
Cy
```

### Path modes and selectors

A pattern without a selector returns every path (`ALL WALK PATHS`). A *walk* may repeat
vertices and edges. These modes restrict that:

| Mode | Rule |
| --- | --- |
| `WALK` | Vertices and edges may repeat. This is the default. |
| `TRAIL` | An edge may not repeat. |
| `ACYCLIC` | A vertex may not repeat. |
| `SIMPLE` | A vertex may not repeat, except that the first vertex may also be the last. |

`KEEP` after the path chooses the mode and, optionally, a selector:

| Clause | Meaning |
| --- | --- |
| `KEEP ALL [mode] PATHS` | Every path in the mode. |
| `KEEP ANY SHORTEST [mode] PATH` | One shortest path, in hops, for each pair of end vertices. |
| `KEEP SHORTEST k [mode] PATHS` | Up to k shortest paths, for each pair of end vertices. |

Ties between paths of equal length are broken by a documented total order on the stable
element identities, not by storage order, so repeated runs return the same path.

A walk with an unbounded quantifier (`*` or `+`) could be infinite on a graph with a cycle,
so Cassie rejects it with `0A000`, even with a shortest selector. Use a bounded
quantifier, or an unbounded one together with `TRAIL`, `ACYCLIC` or `SIMPLE`. The graph above
has the cycle Ada, Cy, Di, Ada:

<!-- example: mode-unbounded-walk -->
```sql
SELECT t.reached
FROM GRAPH_TABLE (
  social MATCH (a IS person WHERE a.name = 'Ada') -[IS knows]->* (b IS person)
  COLUMNS (b.name AS reached)
) AS t
```
```text sqlstate
0A000
```

With `ACYCLIC`, the same pattern terminates. The query below counts how many acyclic paths
reach each person; Cy has two:

<!-- example: mode-acyclic-closure -->
```sql
SELECT t.reached, count(*) AS paths
FROM GRAPH_TABLE (
  social MATCH (a IS person WHERE a.name = 'Ada') -[IS knows]->* (b IS person) KEEP ALL ACYCLIC PATHS
  COLUMNS (b.name AS reached)
) AS t
GROUP BY t.reached
ORDER BY t.reached
```
```text result
Ada | 1
Ben | 1
Cy | 2
Di | 2
Eli | 1
```

`ANY SHORTEST PATH` returns one row per reachable end vertex. A bounded quantifier keeps the
walk finite. Ada reaches herself again by the three-edge cycle:

<!-- example: selector-any-shortest -->
```sql
SELECT t.reached
FROM GRAPH_TABLE (
  social MATCH (a IS person WHERE a.name = 'Ada') -[IS knows]->{1,4} (b IS person) KEEP ANY SHORTEST PATH
  COLUMNS (b.name AS reached)
) AS t
ORDER BY t.reached
```
```text result
Ada
Ben
Cy
Di
Eli
```

### 3.4 Predicates

There are three places for a condition, and they differ in when they apply.

1. **Inline** in a vertex or edge pattern, `(b IS person WHERE b.team = 'ops')` or
   `-[e IS knows WHERE e.active]->`. It constrains the search itself: only elements that
   satisfy it can be part of a match. An inline predicate on an edge that is repeated by a
   quantifier is checked for each repeated edge.
2. In the **pattern**, after the path and its `KEEP` clause, `MATCH ... KEEP ... WHERE
   condition`. It applies to complete paths after the selector has chosen them, and so it
   can only discard selected paths. It never substitutes a different path.
3. In the **outer** query, `WHERE` after `GRAPH_TABLE (...)`. It filters the rows that
   `GRAPH_TABLE` produced. Cassie does not push it into the search.

Filtering the search with an inline predicate:

<!-- example: predicate-inline-edge -->
```sql
SELECT t.friend
FROM GRAPH_TABLE (
  social MATCH (a IS person WHERE a.name = 'Ada') -[e IS knows WHERE e.active]-> (b IS person)
  COLUMNS (b.name AS friend)
) AS t
ORDER BY t.friend
```
```text result
Ben
```

The inline predicate changes which path is shortest. Ada follows Cy directly through an
inactive edge. A shortest path that is allowed to use that edge has one edge and two steps,
while a shortest path restricted to active edges goes through Ben and has three steps:

<!-- example: predicate-shortest-any-edge -->
```sql
SELECT count(*) AS steps
FROM GRAPH_TABLE (
  social MATCH p = (a IS person WHERE a.name = 'Ada') -[e IS knows]->{1,3} (b IS person WHERE b.name = 'Cy')
  KEEP ANY SHORTEST PATH
  COLUMNS (p AS route)
) AS g
CROSS JOIN LATERAL cassie_graph.path_steps(g.route) AS s
```
```text result
2
```

<!-- example: predicate-shortest-active-edge -->
```sql
SELECT count(*) AS steps
FROM GRAPH_TABLE (
  social MATCH p = (a IS person WHERE a.name = 'Ada') -[e IS knows WHERE e.active]->{1,3} (b IS person WHERE b.name = 'Cy')
  KEEP ANY SHORTEST PATH
  COLUMNS (p AS route)
) AS g
CROSS JOIN LATERAL cassie_graph.path_steps(g.route) AS s
```
```text result
3
```

A pattern-level `WHERE` filters after selection. Here the shortest path to each person is
chosen first, and then only the paths that end in the `ops` team are kept:

<!-- example: predicate-pattern-where -->
```sql
SELECT t.reached
FROM GRAPH_TABLE (
  social MATCH (a IS person WHERE a.name = 'Ada') -[IS knows]->{1,4} (b IS person) KEEP ANY SHORTEST PATH WHERE b.team = 'ops'
  COLUMNS (b.name AS reached)
) AS t
ORDER BY t.reached
```
```text result
Cy
Di
```

An outer `WHERE` filters the produced rows:

<!-- example: predicate-outer-where -->
```sql
SELECT t.reached
FROM GRAPH_TABLE (
  social MATCH (a IS person WHERE a.name = 'Ada') -[IS knows]->{1,2} (b IS person)
  COLUMNS (b.name AS reached, b.team AS team)
) AS t
WHERE t.team = 'ops'
ORDER BY t.reached
```
```text result
Cy
Cy
Di
```

Predicates follow SQL's three-valued logic: only `TRUE` satisfies a condition, so a `NULL`
property never matches an equality test.

### 3.5 Parameters

Statement parameters work anywhere a literal does, including inline predicates. The values
are bound, not pasted into the SQL text. This example is run with the parameter `1`:

<!-- example: parameter-start params=1 -->
```sql
SELECT t.friend
FROM GRAPH_TABLE (
  social MATCH (a IS person WHERE a.id = $1) -[IS knows]-> (b IS person)
  COLUMNS (b.name AS friend)
) AS t
ORDER BY t.friend
```
```text result
Ben
Cy
```

Quantifier bounds, `KEEP` counts and mode names are part of the statement and cannot be
parameters.

### 3.6 Composing with SQL

`GRAPH_TABLE` is a normal row source. It joins, aggregates and appears in subqueries. A
`LATERAL` graph source can read columns of the tables to its left, so one statement can run a
pattern for each outer row. Inside a `LATERAL` pattern, a pattern variable that has the same
name as an outer relation hides that relation.

<!-- example: compose-lateral -->
```sql
SELECT p.name, f.friend
FROM people p
CROSS JOIN LATERAL GRAPH_TABLE (
  social MATCH (a IS person WHERE a.id = p.id) -[IS knows]-> (b IS person)
  COLUMNS (b.name AS friend)
) AS f
WHERE p.team = 'ops'
ORDER BY p.name, f.friend
```
```text result
Cy | Di
Di | Ada
```

<!-- example: compose-aggregate -->
```sql
SELECT t.followee, count(*) AS followers
FROM GRAPH_TABLE (
  social MATCH (a IS person) -[IS knows]-> (b IS person)
  COLUMNS (b.name AS followee)
) AS t
GROUP BY t.followee
ORDER BY followers DESC, t.followee
```
```text result
Cy | 2
Ada | 1
Ben | 1
Di | 1
Eli | 1
```

<!-- example: compose-join -->
```sql
SELECT p.name, t.reached
FROM people p
JOIN GRAPH_TABLE (
  social MATCH (a IS person) -[IS knows]-> (b IS person)
  COLUMNS (a.id AS source_id, b.name AS reached)
) AS t ON t.source_id = p.id
WHERE p.name = 'Ben'
ORDER BY t.reached
```
```text result
Ben | Cy
Ben | Eli
```

A graph source is computed completely before its first row is returned. It is never
streamed, so a query that fails part-way returns no rows.

### 3.7 Joining two patterns on a shared vertex

<!-- world: social -->

Two comma-separated patterns match together when they name the same vertex variable. The
variable stands for one vertex identity (its mapping and key), so the patterns meet at the
same vertex and nowhere else. Ada (1) and Ben (2) both follow Cy:

<!-- example: shared-common-neighbor -->
```sql
SELECT t.neighbor
FROM GRAPH_TABLE (
  social MATCH (a IS person WHERE a.id = 1) -[IS knows]-> (n IS person),
               (b IS person WHERE b.id = 2) -[IS knows]-> (n)
  COLUMNS (n.name AS neighbor)
) AS t
ORDER BY t.neighbor
```
```text result
Cy
```

Each combination of one path from each pattern is one row, so parallel edges multiply. A
condition that relates the two patterns goes in the graph-pattern `WHERE`, which sees the
variables of both; an inline predicate sees only its own pattern. Counting the people each
pair follows in common is ordinary SQL over the result:

<!-- example: shared-common-neighbor-count -->
```sql
SELECT t.a_name, t.b_name, COUNT(*)
FROM GRAPH_TABLE (
  social MATCH (a IS person) -[IS knows]-> (n IS person),
               (b IS person) -[IS knows]-> (n)
  WHERE a.id < b.id
  COLUMNS (a.name AS a_name, b.name AS b_name, n.id AS n)
) AS t
GROUP BY t.a_name, t.b_name
ORDER BY t.a_name, t.b_name
```
```text result
Ada | Ben | 1
```

A reachability pattern can feed a second pattern that starts at the shared vertex. Here the
people two follows away from Ada who also follow someone in `ops`:

<!-- example: shared-reachable-then-follows -->
```sql
SELECT t.reviewer, t.target
FROM GRAPH_TABLE (
  social MATCH (a IS person WHERE a.id = 1) -[IS knows]->{2} (r IS person),
               (r) -[IS knows]-> (o IS person WHERE o.team = 'ops')
  COLUMNS (r.name AS reviewer, o.name AS target)
) AS t
ORDER BY t.reviewer, t.target
```
```text result
Cy | Di
```

At most two patterns can be listed, and every pattern after the first must name a vertex
variable an earlier pattern names; patterns are never combined as a cross product, and a path
variable cannot be used with a list. Only vertex variables join patterns. Every limit has a
deterministic error in section 11. `GRAPH_TABLE` still computes the complete result before
returning a row, within the query memory budget, so a join that is too large fails with
`54000` and returns nothing.

## 4. Paths as values

Binding a path variable, `p = (...) -[...]-> (...)`, and projecting it returns the whole path as
one value of the type `cassie_graph.graph_path`. A path has three ordered arrays: the
vertices, the edges between them and the direction in which each edge was followed
(`1` for source to destination, `2` for the reverse). A path of one vertex and no edges is
the zero-hop path.

The path is an identity value, not a snapshot of properties. Each element is a
`cassie_graph.graph_element_ref` that names the database, graph, mapping and typed key of the
vertex or edge, and the key is a list of `cassie_graph.graph_key_part` values. The three types
live in the reserved schema `cassie_graph`, which Cassie owns. You can discover them in
`pg_type` and use them as parameter and result types in a PostgreSQL driver. If your driver
cannot decode named composite arrays, use the functions below, which return scalar columns.

| Function | Result |
| --- | --- |
| `cassie_graph.path_steps(path)` | One row per vertex: `position` from 0, `direction`, `vertex`, and the `edge` that led to it. The first step has no edge and no direction. |
| `cassie_graph.key_parts(reference)` | One row per key component: `ordinal` from 1, `type_tag`, `integer_value`, `text_value` and `uuid_value`. |
| `cassie_graph.path_to_jsonb(path)` | A version 1 JSON document with `version`, `vertices`, `edges` and `directions`. Key values are strings so that a `BIGINT` never loses precision. |
| `cassie_graph.key_part(value)` | Builds one key component from a `smallint`, `integer`, `bigint`, `text` or `uuid` value. |
| `cassie_graph.vertex_ref(graph, mapping, key_parts)` | The reference to an existing vertex, by graph name, mapping alias and key. |
| `cassie_graph.edge_ref(graph, mapping, key_parts)` | The reference to an existing edge. |

`type_tag` is 1 for `smallint`, 2 for `integer`, 3 for `bigint`, 4 for `text` and 5 for
`uuid`. Exactly one of the three value columns is not null.

Steps of a path, from Ada to Cy over active edges:

<!-- example: path-steps -->
```sql
SELECT s.position, s.direction
FROM GRAPH_TABLE (
  social MATCH p = (a IS person WHERE a.name = 'Ada') -[e IS knows WHERE e.active]->{2} (b IS person WHERE b.name = 'Cy')
  COLUMNS (p AS route)
) AS g
CROSS JOIN LATERAL cassie_graph.path_steps(g.route) AS s
ORDER BY s.position
```
```text result
0 | NULL
1 | 1
2 | 1
```

`vertex_ref` builds the reference of a known vertex, and `key_parts` takes it apart:

<!-- example: path-key-parts -->
```sql
SELECT k.ordinal, k.type_tag, k.integer_value
FROM cassie_graph.key_parts(
  cassie_graph.vertex_ref('social', 'person', ARRAY[cassie_graph.key_part(3::bigint)])
) AS k
```
```text result
1 | 3 | 3
```

Because references compare by identity, you can use `vertex_ref` to locate a vertex inside a
path. This returns the position at which Cy appears in the path above:

<!-- example: path-find-vertex -->
```sql
SELECT s.position
FROM GRAPH_TABLE (
  social MATCH p = (a IS person WHERE a.name = 'Ada') -[e IS knows WHERE e.active]->{2} (b IS person WHERE b.name = 'Cy')
  COLUMNS (p AS route)
) AS g
CROSS JOIN LATERAL cassie_graph.path_steps(g.route) AS s
WHERE s.vertex = cassie_graph.vertex_ref('social', 'person', ARRAY[cassie_graph.key_part(3::bigint)])
```
```text result
2
```

`path_to_jsonb` exports a path for an application that does not decode composites. The
directions array shows how each edge was followed:

<!-- example: path-to-jsonb -->
```sql
SELECT cassie_graph.path_to_jsonb(g.route) -> 'directions' AS directions
FROM GRAPH_TABLE (
  social MATCH p = (a IS person WHERE a.name = 'Cy') <-[e IS knows]- (b IS person WHERE b.name = 'Ben')
  COLUMNS (p AS route)
) AS g
```
```text result
[2]
```

A path is a detached value. Constructing one, through a `ROW` cast or a parameter, does not
prove that its edges connect its vertices in the current data. Cassie validates its shape
(tags, arrays and lengths) wherever it enters the system and rejects a malformed value with
`22P02`, `22P03` or `22023`. A reference to a graph that does not exist, or that was
dropped and recreated, returns `42704` instead of resolving to another object. References
are not permission tokens: reading data through them still needs `SELECT` on the tables.

## 5. Weighted paths

`graph_weighted_paths` finds the cheapest simple paths between two vertices, using a numeric
edge property as the cost. Call it with named arguments for readability. Arguments after the
first named one must also be named; omitted trailing arguments use their defaults.

| Argument | Type | Meaning |
| --- | --- | --- |
| `graph` | text | Graph name, optionally schema-qualified. |
| `source`, `target` | `cassie_graph.graph_element_ref` | Vertex references. |
| `weight_property` | text | An edge property of an exact numeric or integer type. |
| `max_hops` | integer | Required, not negative. Longest path in edges. |
| `direction` | text | `'out'` (default), `'in'` or `'both'`. |
| `edge_labels` | text[] | Edge labels to traverse. `NULL` (default) means all; an empty array means none. |
| `max_cost` | numeric | Optional inclusive cost limit. |
| `k` | integer | Number of paths to return, default 1. |

Each row has `path_rank` (1 for the cheapest), `cost`, the `path` as a `graph_path`, and
`edge_costs`, one validated cost per traversed edge in path order. Equal-cost paths are ranked
by the same stable identity order that `GRAPH_TABLE` uses. Paths are `SIMPLE`. Only the first
vertex may repeat, and only as the last vertex. When source and target are the same vertex,
the zero-hop path has cost zero.

Suppose the selected edges are `A -> B` and `B -> A`, each with cost zero. With source and
target both `A`, `direction => 'out'`, `max_hops => 2` and `k => 2`, the zero-hop path
`[A]` ranks first and the closing cycle `[A, B, A]` ranks second. Both paths have cost zero.

The functions `cassie_graph.weighted_path_steps(path, edge_costs)` expand a result into steps
and add the running total in `cumulative_cost`.

### The weight rule

Before it returns anything, `graph_weighted_paths` checks every edge of the selected edge
mappings that is visible in the statement's snapshot. A `NULL`, negative or non-finite weight
anywhere fails the call with `22023`, even when the cheapest path would not touch that edge.
`FLOAT` and `DOUBLE PRECISION` weights are rejected at binding with `42804`, because rounding
would make costs unreliable; use `NUMERIC` or an integer type. Costs add up as `NUMERIC`
without rounding, and an unrepresentable total returns `22003` with no partial result.
Because it validates all weights, the call reads every row of the selected edge tables once,
decoding only each row's key, endpoint and weight columns.

The worked example in [section 6.4](#64-weighted-routes) shows a call and the error.

## 6. Worked examples

Each example starts from ordinary projector tables and ends with a query.

### 6.1 Authorization reachability

<!-- world: access -->

Who can open which document? Users belong to groups, groups can belong to other groups, and
groups or users are granted access to documents. A document is reachable for a user when a
chain of memberships leads to a principal that holds a grant.

<!-- example: access-create-principals -->
```sql
CREATE TABLE principals (id BIGINT PRIMARY KEY, name TEXT NOT NULL, kind TEXT NOT NULL)
```

<!-- example: access-insert-principals -->
```sql
INSERT INTO principals VALUES (1, 'alice', 'user'), (2, 'bob', 'user'), (10, 'eng', 'group'), (11, 'staff', 'group'), (12, 'admins', 'group')
```

<!-- example: access-create-memberships -->
```sql
CREATE TABLE memberships (id BIGINT PRIMARY KEY, member_id BIGINT NOT NULL, group_id BIGINT NOT NULL)
```

<!-- example: access-insert-memberships -->
```sql
INSERT INTO memberships VALUES (1, 1, 10), (2, 2, 10), (3, 10, 11), (4, 2, 12)
```

<!-- example: access-create-documents -->
```sql
CREATE TABLE documents (id BIGINT PRIMARY KEY, title TEXT NOT NULL)
```

<!-- example: access-insert-documents -->
```sql
INSERT INTO documents VALUES (100, 'handbook'), (101, 'payroll'), (102, 'roadmap')
```

<!-- example: access-create-grants -->
```sql
CREATE TABLE grants (id BIGINT PRIMARY KEY, principal_id BIGINT NOT NULL, document_id BIGINT NOT NULL, level TEXT NOT NULL)
```

<!-- example: access-insert-grants -->
```sql
INSERT INTO grants VALUES (1, 11, 100, 'read'), (2, 10, 102, 'write'), (3, 12, 101, 'write')
```

<!-- example: access-create-graph -->
```sql
CREATE PROPERTY GRAPH access
  VERTEX TABLES (
    principals AS principal KEY (id) LABEL principal PROPERTIES (id, name, kind),
    documents AS document KEY (id) LABEL document PROPERTIES (id, title)
  )
  EDGE TABLES (
    memberships AS member_of KEY (id)
      SOURCE KEY (member_id) REFERENCES principal (id)
      DESTINATION KEY (group_id) REFERENCES principal (id)
      LABEL member_of PROPERTIES (id),
    grants AS grant_edge KEY (id)
      SOURCE KEY (principal_id) REFERENCES principal (id)
      DESTINATION KEY (document_id) REFERENCES document (id)
      LABEL grants PROPERTIES (level)
  )
```

The query starts at the user, climbs zero to three membership edges, and follows one grant.
Zero memberships covers a grant made directly to the user. `DISTINCT` removes repeats when
more than one chain reaches the same document:

<!-- example: access-reachable-documents -->
```sql
SELECT DISTINCT t.title, t.level
FROM GRAPH_TABLE (
  access MATCH (u IS principal WHERE u.name = 'bob') -[IS member_of]->{0,3} (g IS principal) -[x IS grants]-> (d IS document)
  COLUMNS (d.title AS title, x.level AS level)
) AS t
ORDER BY t.title
```
```text result
handbook | read
payroll | write
roadmap | write
```

The same pattern read backwards answers the audit question, who can open this document:

<!-- example: access-who-can-open -->
```sql
SELECT DISTINCT t.who
FROM GRAPH_TABLE (
  access MATCH (d IS document WHERE d.title = 'handbook') <-[IS grants]- (g IS principal) <-[IS member_of]-{0,3} (u IS principal WHERE u.kind = 'user')
  COLUMNS (u.name AS who)
) AS t
ORDER BY t.who
```
```text result
alice
bob
```

The bound of three bounds the depth of nested groups that the query follows. Choose it from
the deepest nesting your data allows; a membership chain longer than the bound is not
followed.

### 6.2 Hierarchy ancestry

<!-- world: org -->

A reporting tree stores one row per employee that names the manager. The chain of command
of an employee is the set of all ancestors.

<!-- example: org-create-employees -->
```sql
CREATE TABLE employees (id BIGINT PRIMARY KEY, name TEXT NOT NULL)
```

<!-- example: org-insert-employees -->
```sql
INSERT INTO employees VALUES (1, 'Ines'), (2, 'Jo'), (3, 'Kai'), (4, 'Lee'), (5, 'Moe'), (6, 'Ned')
```

<!-- example: org-create-reports -->
```sql
CREATE TABLE reports (id BIGINT PRIMARY KEY, employee_id BIGINT NOT NULL, manager_id BIGINT NOT NULL)
```

<!-- example: org-insert-reports -->
```sql
INSERT INTO reports VALUES (1, 2, 1), (2, 3, 1), (3, 4, 2), (4, 5, 2), (5, 6, 4)
```

<!-- example: org-create-graph -->
```sql
CREATE PROPERTY GRAPH org
  VERTEX TABLES (employees AS employee KEY (id) LABEL employee PROPERTIES (id, name))
  EDGE TABLES (
    reports AS reports_to KEY (id)
      SOURCE KEY (employee_id) REFERENCES employee (id)
      DESTINATION KEY (manager_id) REFERENCES employee (id)
      LABEL reports_to PROPERTIES (id)
  )
```

An unbounded `+` with `ACYCLIC` follows the chain to the top and stops, whatever its depth:

<!-- example: org-chain-of-command -->
```sql
SELECT t.manager
FROM GRAPH_TABLE (
  org MATCH (e IS employee WHERE e.name = 'Ned') -[IS reports_to]->+ (m IS employee) KEEP ALL ACYCLIC PATHS
  COLUMNS (m.name AS manager)
) AS t
ORDER BY t.manager
```
```text result
Ines
Jo
Lee
```

Reading the edge backwards lists everyone below a manager, at any depth:

<!-- example: org-everyone-below -->
```sql
SELECT t.report
FROM GRAPH_TABLE (
  org MATCH (m IS employee WHERE m.name = 'Jo') <-[IS reports_to]-+ (e IS employee) KEEP ALL ACYCLIC PATHS
  COLUMNS (e.name AS report)
) AS t
ORDER BY t.report
```
```text result
Lee
Moe
Ned
```

To get depth, count the steps of the projected path. Ned is three levels below Ines:

<!-- example: org-depth -->
```sql
SELECT count(*) - 1 AS levels
FROM GRAPH_TABLE (
  org MATCH p = (e IS employee WHERE e.name = 'Ned') -[IS reports_to]->+ (m IS employee WHERE m.name = 'Ines') KEEP ANY SHORTEST ACYCLIC PATH
  COLUMNS (p AS route)
) AS g
CROSS JOIN LATERAL cassie_graph.path_steps(g.route) AS s
```
```text result
3
```

Unbounded traversal is limited by the query memory budget. A very deep or very wide
traversal that exceeds it fails with `54000` rather than running without limit.

### 6.3 Friends of friends

<!-- world: social -->

With the `social` graph from sections 2 and 3, friends of friends are the people exactly two
edges away. Excluding the people the start vertex already follows needs an anti-join, which
is ordinary SQL:

<!-- example: fof-exact-two -->
```sql
SELECT DISTINCT t.suggestion
FROM GRAPH_TABLE (
  social MATCH (a IS person WHERE a.name = 'Ada') -[IS knows]->{2} (b IS person)
  COLUMNS (b.id AS suggestion_id, b.name AS suggestion)
) AS t
WHERE t.suggestion_id NOT IN (
  SELECT f.followee_id FROM follows f WHERE f.follower_id = 1
)
ORDER BY t.suggestion
```
```text result
Di
Eli
```

### 6.4 Weighted routes

<!-- world: routes -->

Roads connect depots, and each road has a length in kilometres. The graph maps the two
tables, and `graph_weighted_paths` finds the three shortest routes by total length.

<!-- example: routes-create-places -->
```sql
CREATE TABLE places (id BIGINT PRIMARY KEY, name TEXT NOT NULL)
```

<!-- example: routes-insert-places -->
```sql
INSERT INTO places VALUES (1, 'Depot'), (2, 'Mill'), (3, 'Port'), (4, 'Town'), (5, 'Yard')
```

<!-- example: routes-create-roads -->
```sql
CREATE TABLE roads (id BIGINT PRIMARY KEY, a BIGINT NOT NULL, b BIGINT NOT NULL, km NUMERIC(8,2))
```

<!-- example: routes-insert-roads -->
```sql
INSERT INTO roads VALUES (1, 1, 2, 4.0), (2, 1, 3, 10.0), (3, 2, 3, 5.5), (4, 2, 4, 12.0), (5, 3, 4, 3.0), (6, 4, 5, 2.0)
```

<!-- example: routes-create-graph -->
```sql
CREATE PROPERTY GRAPH network
  VERTEX TABLES (places AS place KEY (id) LABEL place PROPERTIES (id, name))
  EDGE TABLES (
    roads AS road KEY (id)
      SOURCE KEY (a) REFERENCES place (id)
      DESTINATION KEY (b) REFERENCES place (id)
      LABEL road PROPERTIES (km)
  )
```

<!-- example: routes-cheapest -->
```sql
SELECT w.path_rank, w.cost::text AS km
FROM graph_weighted_paths(
  graph => 'network',
  source => cassie_graph.vertex_ref('network', 'place', ARRAY[cassie_graph.key_part(1::bigint)]),
  target => cassie_graph.vertex_ref('network', 'place', ARRAY[cassie_graph.key_part(5::bigint)]),
  weight_property => 'km',
  max_hops => 5,
  k => 3
) AS w
ORDER BY w.path_rank
```
```text result
1 | 14.50
2 | 15.00
3 | 18.00
```

`direction => 'both'` lets a route use roads in either direction, and `max_cost` caps the
total. `weighted_path_steps` lists the stops of the best route with the running distance:

<!-- example: routes-steps -->
```sql
SELECT s.position, s.cumulative_cost::text AS km
FROM graph_weighted_paths(
  graph => 'network',
  source => cassie_graph.vertex_ref('network', 'place', ARRAY[cassie_graph.key_part(1::bigint)]),
  target => cassie_graph.vertex_ref('network', 'place', ARRAY[cassie_graph.key_part(5::bigint)]),
  weight_property => 'km',
  max_hops => 5
) AS w
CROSS JOIN LATERAL cassie_graph.weighted_path_steps(w.path, w.edge_costs) AS s
ORDER BY s.position
```
```text result
0 | 0
1 | 4.00
2 | 9.50
3 | 12.50
4 | 14.50
```

<!-- example: routes-both-directions -->
```sql
SELECT w.path_rank, w.cost::text AS km
FROM graph_weighted_paths(
  graph => 'network',
  source => cassie_graph.vertex_ref('network', 'place', ARRAY[cassie_graph.key_part(5::bigint)]),
  target => cassie_graph.vertex_ref('network', 'place', ARRAY[cassie_graph.key_part(1::bigint)]),
  weight_property => 'km',
  max_hops => 5,
  direction => 'both',
  max_cost => 16
) AS w
ORDER BY w.path_rank
```
```text result
1 | 14.50
```

A negative weight anywhere in the edge table makes every call fail, even for a route that
never uses that road:

<!-- example: routes-insert-bad-road -->
```sql
INSERT INTO roads VALUES (7, 5, 1, -1.0)
```

<!-- example: routes-negative-weight -->
```sql
SELECT w.path_rank
FROM graph_weighted_paths(
  graph => 'network',
  source => cassie_graph.vertex_ref('network', 'place', ARRAY[cassie_graph.key_part(1::bigint)]),
  target => cassie_graph.vertex_ref('network', 'place', ARRAY[cassie_graph.key_part(2::bigint)]),
  weight_property => 'km',
  max_hops => 1
) AS w
```
```text sqlstate
22023
```

## 7. Transactions and visibility

<!-- world: social -->

A graph query reads the mapped tables in the transaction view of its own statement. Inside a
transaction it sees the transaction's own uncommitted inserts, updates and deletes, including
rows written before a savepoint that has not been rolled back. Other sessions see only
committed rows. The following sequence uses two sessions, `writer` and `observer`. Eli follows
nobody yet:

<!-- example: tx-before session=observer -->
```sql
SELECT count(*) AS followed
FROM GRAPH_TABLE (
  social MATCH (a IS person WHERE a.name = 'Eli') -[IS knows]-> (b IS person)
  COLUMNS (b.id AS id)
) AS t
```
```text result
0
```

<!-- example: tx-begin session=writer -->
```sql
BEGIN
```

<!-- example: tx-insert-edge session=writer -->
```sql
INSERT INTO follows VALUES (7, 5, 1, TRUE, 1)
```

The writer sees its own edge, and the observer does not:

<!-- example: tx-writer-sees session=writer -->
```sql
SELECT count(*) AS followed
FROM GRAPH_TABLE (
  social MATCH (a IS person WHERE a.name = 'Eli') -[IS knows]-> (b IS person)
  COLUMNS (b.id AS id)
) AS t
```
```text result
1
```

<!-- example: tx-reader-hidden session=observer -->
```sql
SELECT count(*) AS followed
FROM GRAPH_TABLE (
  social MATCH (a IS person WHERE a.name = 'Eli') -[IS knows]-> (b IS person)
  COLUMNS (b.id AS id)
) AS t
```
```text result
0
```

<!-- example: tx-commit session=writer -->
```sql
COMMIT
```

<!-- example: tx-reader-after session=observer -->
```sql
SELECT count(*) AS followed
FROM GRAPH_TABLE (
  social MATCH (a IS person WHERE a.name = 'Eli') -[IS knows]-> (b IS person)
  COLUMNS (b.id AS id)
) AS t
```
```text result
1
```

A rolled-back transaction leaves no trace in later graph queries. An edge whose endpoint
vertex does not exist is not an edge yet. This edge points at person 99:

<!-- example: tx-dangling-edge -->
```sql
INSERT INTO follows VALUES (8, 5, 99, TRUE, 1)
```

<!-- example: tx-dangling-hidden -->
```sql
SELECT count(*) AS followed
FROM GRAPH_TABLE (
  social MATCH (a IS person WHERE a.name = 'Eli') -[IS knows]-> (b IS person)
  COLUMNS (b.id AS id)
) AS t
```
```text result
1
```

Inserting the vertex makes the edge visible; no change to the graph is needed:

<!-- example: tx-insert-endpoint -->
```sql
INSERT INTO people VALUES (99, 'Zed', 'eng')
```

<!-- example: tx-dangling-visible -->
```sql
SELECT count(*) AS followed
FROM GRAPH_TABLE (
  social MATCH (a IS person WHERE a.name = 'Eli') -[IS knows]-> (b IS person)
  COLUMNS (b.id AS id)
) AS t
```
```text result
2
```

A statement reads one consistent view: vertex properties, edge properties and endpoint
resolution all come from the same snapshot, so a concurrent commit cannot show a path
whose edge exists but whose endpoint does not.

## 8. Permissions

<!-- world: permissions -->

A property graph grants nothing. To read a graph a role needs `SELECT` on every table the
pattern can read, which includes the endpoint vertex tables of every edge mapping it uses,
and `USAGE` on their schema. Having the right to the graph name alone gives no access.
`CREATE`, `ALTER` and `DROP PROPERTY GRAPH` are administrator statements, like other DDL, and
fail with `42501` for other roles.

The examples below use a role `reader`, which an administrator created with a password.
They run as `reader` and as `root`. The tables are the ones from section 2.

<!-- example: perm-create-people -->
```sql
CREATE TABLE people (id BIGINT PRIMARY KEY, name TEXT NOT NULL)
```

<!-- example: perm-create-follows -->
```sql
CREATE TABLE follows (id BIGINT PRIMARY KEY, a BIGINT NOT NULL, b BIGINT NOT NULL)
```

<!-- example: perm-insert-people -->
```sql
INSERT INTO people VALUES (1, 'Ada'), (2, 'Ben')
```

<!-- example: perm-insert-follows -->
```sql
INSERT INTO follows VALUES (1, 1, 2)
```

<!-- example: perm-create-graph -->
```sql
CREATE PROPERTY GRAPH circle
  VERTEX TABLES (people AS person KEY (id) LABEL person PROPERTIES (id, name))
  EDGE TABLES (
    follows AS knows KEY (id)
      SOURCE KEY (a) REFERENCES person (id)
      DESTINATION KEY (b) REFERENCES person (id)
      LABEL knows PROPERTIES (id)
  )
```

Without privileges, the query fails, and so does a lookup of a vertex reference. The error
is the same whether or not the vertex exists, so it does not reveal data:

<!-- example: perm-denied session=reader -->
```sql
SELECT t.friend
FROM GRAPH_TABLE (
  circle MATCH (a IS person WHERE a.id = 1) -[IS knows]-> (b IS person)
  COLUMNS (b.name AS friend)
) AS t
```
```text sqlstate
42501
```

<!-- example: perm-denied-reference session=reader -->
```sql
SELECT cassie_graph.vertex_ref('circle', 'person', ARRAY[cassie_graph.key_part(1::bigint)])
```
```text sqlstate
42501
```

Discovery follows the same rule. A role that cannot read all of a graph's tables does not see
the graph listed:

<!-- example: perm-hidden-graph session=reader -->
```sql
SELECT count(*) FROM information_schema.property_graphs
```
```text result
0
```

After the grants, the reader can query the graph and see it listed:

<!-- example: perm-grant-schema session=root -->
```sql
GRANT USAGE ON SCHEMA public TO reader
```

<!-- example: perm-grant-people session=root -->
```sql
GRANT SELECT ON TABLE public.people TO reader
```

<!-- example: perm-grant-follows session=root -->
```sql
GRANT SELECT ON TABLE public.follows TO reader
```

<!-- example: perm-allowed session=reader -->
```sql
SELECT t.friend
FROM GRAPH_TABLE (
  circle MATCH (a IS person WHERE a.id = 1) -[IS knows]-> (b IS person)
  COLUMNS (b.name AS friend)
) AS t
```
```text result
Ben
```

<!-- example: perm-visible-graph session=reader -->
```sql
SELECT property_graph_name FROM information_schema.property_graphs
```
```text result
circle
```

<!-- example: perm-no-ddl session=reader -->
```sql
DROP PROPERTY GRAPH circle
```
```text sqlstate
42501
```

## 9. Discovery

<!-- world: social -->

Standard `information_schema` views describe each graph, following the SQL/PGQ view names:
`property_graphs`, `pg_element_tables`, `pg_element_table_key_columns`,
`pg_edge_table_components`, `pg_element_table_labels`, `pg_element_table_properties` and
`pg_label_properties`. The view `pg_catalog.pg_property_graph_identities` adds the durable
graph, database and mapping identifiers that the standard views cannot carry. A graph is
listed only to a session that can read all of its mapped tables. `property_graphs` also
carries `property_graph_definition`, the exact `CREATE PROPERTY GRAPH` statement that
recreates the graph ([section 14](#14-recreating-graphs-in-a-database-clone)). The administrative REST
catalog (`GET /api/v1/admin/catalog`, also served at `/api/v1/admin/query/schema`) lists the
graphs of the database in a `property_graphs` section; see the [OpenAPI document](../public/openapi.yml).

<!-- example: discovery-graphs -->
```sql
SELECT property_graph_name FROM information_schema.property_graphs ORDER BY property_graph_name
```
```text result
social
```

<!-- example: discovery-elements -->
```sql
SELECT element_table_alias, element_table_kind, table_name
FROM information_schema.pg_element_tables
WHERE property_graph_name = 'social'
ORDER BY element_table_kind DESC, element_table_alias
```
```text result
person | VERTEX | people
knows | EDGE | follows
```

<!-- example: discovery-label-properties -->
```sql
SELECT label_name, property_name
FROM information_schema.pg_label_properties
WHERE property_graph_name = 'social'
ORDER BY label_name, property_name
```
```text result
knows | active
knows | cost
person | id
person | name
person | team
```

<!-- example: discovery-edge-ends -->
```sql
SELECT element_table_alias, edge_end, edge_table_column_name, vertex_table_alias, vertex_table_column_name
FROM information_schema.pg_edge_table_components
WHERE property_graph_name = 'social'
ORDER BY edge_end
```
```text result
knows | DESTINATION | followee_id | person | id
knows | SOURCE | follower_id | person | id
```

<!-- example: discovery-identities -->
```sql
SELECT element_table_alias, element_table_kind
FROM pg_catalog.pg_property_graph_identities
WHERE property_graph_name = 'social'
ORDER BY element_table_kind DESC
```
```text result
person | VERTEX
knows | EDGE
```

## 10. Lifecycle

<!-- world: lifecycle -->

A graph depends on the tables, key constraints and columns it reads. While a graph maps
them, changes that would break it are rejected, so a mapped graph never silently changes
meaning:

- Dropping a mapped table, or dropping a column that a mapping reads (a key, an endpoint
  or an exposed property column) is rejected with `2BP01` (`dependent_objects_still_exist`).
- Changing the type of such a column, making a key column nullable, and dropping the
  constraint or index that enforces a mapped key (unless another non-partial unique key still
  covers exactly the mapped key columns) are rejected with `2BP01` too.
- `CASCADE` does not remove a graph in this release. `DROP TABLE ... CASCADE`, `DROP COLUMN ...
  CASCADE` and a `DROP SCHEMA ... CASCADE` that would leave a graph of another schema without
  its tables return `0A000`. A `DROP SCHEMA ... CASCADE` drops the graphs that the schema owns.
- Renaming a mapped table, column or schema is allowed. The graph follows the new name and
  keeps its identity. Columns you rename keep their exposed property names.
- A view that selects from `GRAPH_TABLE` depends on its graph: `DROP PROPERTY GRAPH` returns
  `2BP01` while such a view exists.

Drop the dependent graph first, and the table changes then run as usual:

<!-- example: lifecycle-create-nodes -->
```sql
CREATE TABLE sites (id BIGINT PRIMARY KEY, name TEXT NOT NULL)
```

<!-- example: lifecycle-create-links -->
```sql
CREATE TABLE site_links (id BIGINT PRIMARY KEY, a BIGINT NOT NULL, b BIGINT NOT NULL, note TEXT)
```

<!-- example: lifecycle-create-graph -->
```sql
CREATE PROPERTY GRAPH mesh
  VERTEX TABLES (sites AS site KEY (id) LABEL site PROPERTIES (id, name))
  EDGE TABLES (
    site_links AS link KEY (id)
      SOURCE KEY (a) REFERENCES site (id)
      DESTINATION KEY (b) REFERENCES site (id)
      LABEL link PROPERTIES (id)
  )
```

<!-- example: lifecycle-drop-mapped-table -->
```sql
DROP TABLE site_links
```
```text sqlstate
2BP01
```

<!-- example: lifecycle-drop-cascade -->
```sql
DROP TABLE site_links CASCADE
```
```text sqlstate
0A000
```

<!-- example: lifecycle-drop-mapped-column -->
```sql
ALTER TABLE site_links DROP COLUMN a
```
```text sqlstate
2BP01
```

A column that no mapping reads can still change:

<!-- example: lifecycle-drop-unread-column -->
```sql
ALTER TABLE site_links DROP COLUMN note
```

<!-- example: lifecycle-rename-table -->
```sql
ALTER TABLE site_links RENAME TO connections
```

`ALTER PROPERTY GRAPH ... RENAME TO` renames a graph and keeps its identity. Altering a
definition beyond its name, and moving a graph to another schema, return `0A000`. Rename onto
an existing graph is `42710`, and renaming or dropping a graph that does not exist is `42704`:

<!-- example: lifecycle-rename-graph -->
```sql
ALTER PROPERTY GRAPH mesh RENAME TO grid
```

<!-- example: lifecycle-rename-missing -->
```sql
ALTER PROPERTY GRAPH mesh RENAME TO other
```
```text sqlstate
42704
```

<!-- example: lifecycle-drop-graph -->
```sql
DROP PROPERTY GRAPH grid
```

<!-- example: lifecycle-drop-missing -->
```sql
DROP PROPERTY GRAPH grid
```
```text sqlstate
42704
```

<!-- example: lifecycle-drop-if-exists -->
```sql
DROP PROPERTY GRAPH IF EXISTS grid
```

After the graph is gone, the tables are unchanged and can be dropped:

<!-- example: lifecycle-drop-table-after -->
```sql
DROP TABLE connections
```

## 11. Limits and unsupported constructs

<!-- world: social -->

Cassie supports a finite subset of graph patterns. The constructs below are recognized and
rejected with a deterministic error. Nothing is silently reinterpreted.

| Construct or condition | SQLSTATE |
| --- | --- |
| A third comma-separated pattern, patterns that share no vertex variable, a shared edge variable, a path variable with several patterns, pattern unions, graph mutation syntax | `0A000` |
| Unbounded `WALK` (`*`, `+`) without a mode, with or without a selector | `0A000` |
| Parenthesized repetition and nested quantifiers | `0A000` |
| Reading a quantified (group) edge variable outside its own inline `WHERE` | `0A000` |
| A subquery inside a graph predicate or projection, `WITH ORDINALITY` | `0A000` |
| Repeating a quantified edge variable in one pattern | `0A000` |
| A quantifier whose lower bound exceeds its upper bound, or `SHORTEST 0 PATHS` | `22023` |
| A projection that is not `variable.property` and has no `AS` alias | `42601` |
| Two output columns with the same name | `42701` |
| A shared vertex variable that no mapping carries every named label of | `42804` |
| An unknown graph or label | `42704` |
| An unknown variable or property | the binder's usual `42703`/`42P01` errors |
| No `SELECT` privilege on a table the pattern can read | `42501` |
| Query memory budget exceeded by frontier or path state | `54000` |
| Cancellation or statement timeout | `57014` |

<!-- example: limit-two-patterns -->
```sql
SELECT t.x
FROM GRAPH_TABLE (
  social MATCH (a IS person), (b IS person)
  COLUMNS (a.id AS x)
) AS t
```
```text sqlstate
0A000
```

<!-- example: limit-shared-edge-variable -->
```sql
SELECT t.x
FROM GRAPH_TABLE (
  social MATCH (a IS person) -[e IS knows]-> (b IS person),
               (b) -[e IS knows]-> (c IS person)
  COLUMNS (a.id AS x)
) AS t
```
```text sqlstate
0A000
```

<!-- example: limit-bad-bounds -->
```sql
SELECT t.x
FROM GRAPH_TABLE (
  social MATCH (a IS person) -[IS knows]->{3,1} (b IS person)
  COLUMNS (a.id AS x)
) AS t
```
```text sqlstate
22023
```

<!-- example: limit-shortest-zero -->
```sql
SELECT t.x
FROM GRAPH_TABLE (
  social MATCH (a IS person) -[IS knows]->{1,2} (b IS person) KEEP SHORTEST 0 PATHS
  COLUMNS (a.id AS x)
) AS t
```
```text sqlstate
22023
```

<!-- example: limit-missing-alias -->
```sql
SELECT t.x
FROM GRAPH_TABLE (
  social MATCH (a IS person)
  COLUMNS (a.id + 1)
) AS t
```
```text sqlstate
42601
```

<!-- example: limit-duplicate-columns -->
```sql
SELECT t.x
FROM GRAPH_TABLE (
  social MATCH (a IS person)
  COLUMNS (a.id AS x, a.name AS x)
) AS t
```
```text sqlstate
42701
```

<!-- example: limit-unknown-label -->
```sql
SELECT t.x
FROM GRAPH_TABLE (
  social MATCH (a IS planet)
  COLUMNS (a.id AS x)
) AS t
```
```text sqlstate
42704
```

<!-- example: limit-unknown-graph -->
```sql
SELECT t.x
FROM GRAPH_TABLE (
  nowhere MATCH (a IS person)
  COLUMNS (a.id AS x)
) AS t
```
```text sqlstate
42704
```

<!-- example: limit-group-variable -->
```sql
SELECT t.c
FROM GRAPH_TABLE (
  social MATCH (a IS person) -[e IS knows]->{1,2} (b IS person)
  COLUMNS (e.cost AS c)
) AS t
```
```text sqlstate
0A000
```

<!-- example: limit-subquery -->
```sql
SELECT t.x
FROM GRAPH_TABLE (
  social MATCH (a IS person WHERE a.id IN (SELECT 1))
  COLUMNS (a.id AS x)
) AS t
```
```text sqlstate
0A000
```

Other limits to keep in mind:

- Read cost is bounded by counters, not by timings. A graph query expands from its start
  vertices over the ordinary scalar indexes of your tables (no adjacency index is stored),
  so the bounds below hold only when those indexes exist:
  - **Degree and depth.** With a scalar index on the edge table's endpoint columns and a
    start vertex fixed by key equality in the pattern's inline `WHERE`, a traversal reads
    O(sum of the degree of the vertices it visits), at most one neighbourhood per visited
    vertex and edge side, and `{1,n}` visits at most `n` layers. Edges of other, unreachable
    parts of the graph are never read, however many there are.
  - **Without an endpoint index, or with a start that is not fixed by key**, the mapping is
    read in full once per statement: O(rows of that table), and a query over a very large
    table can exceed the query memory budget (`54000`). `EXPLAIN` names the reason.
  - **Weights.** `graph_weighted_paths` first checks the weight of every graph-visible edge
    of the selected edge mappings (see [the weight rule](#the-weight-rule)), by streaming
    each row once and decoding only its key, endpoint and weight columns. That check is
    O(selected edge rows) on every call by contract, even when the search itself touches
    three edges; the search then reads only the neighbourhoods it visits.
  - **References.** `cassie_graph.vertex_ref` and `edge_ref` read the one index entry and row
    their key names through the unique index that backs the mapped key, not the mapped table.
  - `EXPLAIN ANALYZE` reports `graph_reads=` with `index_entries`, `row_fetches`,
    `full_rows`, `overlay_changes`, `cache_hits`, `validation_rows`, `frontier_peak`,
    `edges_admitted` (edges that entered the traversal) and `neighborhoods` (neighbourhood
    expansions). The same counters are exported as `read_paths.graph_*` runtime metrics. The
    cost-curve fixtures are `tests/graph_table/cost_curves.rs` and
    `tests/graph_weighted/cost_curves.rs`. Wall-clock evidence is tracked in the
    [graph qualification record](graph-v060-gap-analysis.md#qualification-evidence).
- Bounds do not make every pattern cheap. Finite `TRAIL`, `ACYCLIC` and `SIMPLE` results can still be
  exponentially numerous on a dense graph. The query memory budget stops such a query with `54000`.
- A graph covers one database. All mapped tables must be ordinary tables of that database.
  Views, materialized views and cross-database tables are not mappable.
- Key components are `SMALLINT`, `INTEGER`, `BIGINT`, `TEXT` (C collation) and `UUID` only.
- A graph source is computed completely before its first row is returned, so a failing
  statement returns no partial rows.
- Properties exposed by one label must have one SQL type across all the mappings the label
  selects, or binding that property fails.
- This is a read-model feature on one node. It does not provide graph mutation, `CASCADE` of
  graph dependencies, distributed execution, graph analytics such as PageRank, or acceleration
  of recursive CTEs.

## 12. The removal of CREATE GRAPH

<!-- world: legacy -->

Earlier builds had a different graph feature: `CREATE GRAPH`, two fixed backing
tables (`<name>_nodes` and `<name>_edges`), and the positional functions `graph_neighbors`,
`graph_expand` and `graph_shortest_path`. That feature was removed in v0.6.0 without
conversion. A store that still holds a legacy definition ignores it and deletes the stored
definition and its adjacency entries at startup. The former node and edge tables remain
ordinary tables, and you can map them with `CREATE PROPERTY GRAPH`.

<!-- example: legacy-create-graph -->
```sql
CREATE GRAPH legacy
```
```text sqlstate
0A000
```

<!-- example: legacy-function -->
```sql
SELECT * FROM graph_neighbors('legacy', 'doc', 'a', 'out', '*', 10)
```
```text sqlstate
42601
```

What to use instead:

| Legacy | Replacement |
| --- | --- |
| `CREATE GRAPH g` with fixed `g_nodes` and `g_edges` tables | `CREATE PROPERTY GRAPH g` over your own vertex and edge tables ([section 2](#2-defining-a-graph)). |
| `graph_neighbors(...)` | A one-hop `GRAPH_TABLE` pattern. |
| `graph_expand(...)` | A quantified pattern with a mode, `-[IS label]->{1,n}`. |
| `graph_shortest_path(...)` | `KEEP ANY SHORTEST PATH` for hops, or `graph_weighted_paths` for weights. |

The removal drops three behaviours that the mapped model does not carry. Edges whose endpoint
rows do not exist are not traversed: add the missing vertex rows. `FLOAT` edge weights are not
accepted: use an exact numeric or integer weight column. A per-row node or edge type value is
replaced by labels on mappings: declare one mapping per type, or one label per mapping.

## 13. Stable identity across projector corrections

<!-- world: identity -->

A projector that maintains an authorization read model often maps the same tables into more
than one graph, keys its vertices by a tenant-qualified composite key, and corrects rows
after the fact. This section is the proven recipe for that workload (issue #1351). It is
qualified by `tests/graph_types/identity_*.rs` over **both** row tables and column-authoritative
tables (`CREATE TABLE ... USING columnar`); the examples below run on row tables, and the
suite repeats every statement of the fixture with `USING columnar` and expects identical
results. The executable parts of the recipe follow; the restart, rename and reinsertion rules
are in [the rules table](#rules-and-error-codes).

### Fixture

Two tenants both hold members `ann` and `bob`, so the display key `code` is not unique: the
identity is the pair `(tenant_id, code)`. Membership 2 parallels membership 1, membership 3 is
a self-loop, membership 5 points at a member that does not exist yet (`zed`, tenant 1) and
membership 7 does the same in tenant 2 (`cy`).

<!-- example: id-create-member -->
```sql
CREATE TABLE member (
  tenant_id BIGINT NOT NULL, code TEXT NOT NULL, label TEXT,
  PRIMARY KEY (tenant_id, code)
)
```

<!-- example: id-create-membership -->
```sql
CREATE TABLE membership (
  id BIGINT PRIMARY KEY, tenant_id BIGINT NOT NULL, src TEXT NOT NULL, dst TEXT NOT NULL
)
```

<!-- example: id-insert-member -->
```sql
INSERT INTO member VALUES
  (1, 'ann', 'Ann one'), (1, 'bob', 'Bob one'), (2, 'ann', 'Ann two'), (2, 'bob', 'Bob two')
```

<!-- example: id-insert-membership -->
```sql
INSERT INTO membership VALUES
  (1, 1, 'ann', 'bob'), (2, 1, 'ann', 'bob'), (3, 1, 'bob', 'bob'), (4, 2, 'ann', 'bob'),
  (5, 1, 'ann', 'zed'), (6, 2, 'bob', 'ann'), (7, 2, 'bob', 'cy')
```

The same tables are mapped into two graphs under two sets of aliases (`m` and `grant_edge`,
`person` and `grants_to`):

<!-- example: id-create-graph-a -->
```sql
CREATE PROPERTY GRAPH authz_a
  VERTEX TABLES (member AS m KEY (tenant_id, code) LABEL member
    PROPERTIES (tenant_id, code, label))
  EDGE TABLES (membership AS grant_edge KEY (id)
    SOURCE KEY (tenant_id, src) REFERENCES m (tenant_id, code)
    DESTINATION KEY (tenant_id, dst) REFERENCES m (tenant_id, code)
    LABEL grants PROPERTIES (id))
```

<!-- example: id-create-graph-b -->
```sql
CREATE PROPERTY GRAPH authz_b
  VERTEX TABLES (member AS person KEY (tenant_id, code) LABEL member
    PROPERTIES (tenant_id, code, label))
  EDGE TABLES (membership AS grants_to KEY (id)
    SOURCE KEY (tenant_id, src) REFERENCES person (tenant_id, code)
    DESTINATION KEY (tenant_id, dst) REFERENCES person (tenant_id, code)
    LABEL grants PROPERTIES (id))
```

### Visible edges against a relational join

Edges 5 and 7 are ordinary rows but not edges yet. The graph and an independent relational
join agree on the five edges that are visible, in both graphs:

<!-- example: id-edges-a -->
```sql
SELECT g.id, g.st, g.sc, g.dt, g.dc
FROM GRAPH_TABLE (
  authz_a MATCH (s IS member) -[e IS grants]-> (d IS member)
  COLUMNS (e.id AS id, s.tenant_id AS st, s.code AS sc, d.tenant_id AS dt, d.code AS dc)
) AS g
ORDER BY g.id
```
```text result
1 | 1 | ann | 1 | bob
2 | 1 | ann | 1 | bob
3 | 1 | bob | 1 | bob
4 | 2 | ann | 2 | bob
6 | 2 | bob | 2 | ann
```

<!-- example: id-edges-b -->
```sql
SELECT g.id, g.st, g.sc, g.dt, g.dc
FROM GRAPH_TABLE (
  authz_b MATCH (s IS member) -[e IS grants]-> (d IS member)
  COLUMNS (e.id AS id, s.tenant_id AS st, s.code AS sc, d.tenant_id AS dt, d.code AS dc)
) AS g
ORDER BY g.id
```
```text result
1 | 1 | ann | 1 | bob
2 | 1 | ann | 1 | bob
3 | 1 | bob | 1 | bob
4 | 2 | ann | 2 | bob
6 | 2 | bob | 2 | ann
```

<!-- example: id-edges-join -->
```sql
SELECT l.id, s.tenant_id, s.code, d.tenant_id, d.code
FROM membership AS l
JOIN member AS s ON s.tenant_id = l.tenant_id AND s.code = l.src
JOIN member AS d ON d.tenant_id = l.tenant_id AND d.code = l.dst
ORDER BY l.id
```
```text result
1 | 1 | ann | 1 | bob
2 | 1 | ann | 1 | bob
3 | 1 | bob | 1 | bob
4 | 2 | ann | 2 | bob
6 | 2 | bob | 2 | ann
```

### Expected identities

An element's identity is its database, graph, mapping and complete typed key. The key of a
vertex here has two components, `bigint` (type tag 3) then `text` (type tag 4):

<!-- example: id-key-parts -->
```sql
SELECT k.ordinal, k.type_tag, k.integer_value, k.text_value
FROM cassie_graph.key_parts(
  cassie_graph.vertex_ref('authz_a', 'm',
    ARRAY[cassie_graph.key_part(2::bigint), cassie_graph.key_part('ann'::text)])
) AS k
ORDER BY k.ordinal
```
```text result
1 | 3 | 2 | NULL
2 | 4 | NULL | ann
```

The display key `ann` of tenant 1 and of tenant 2 are different identities; the same key in
the second graph (a different graph and a different mapping) is a different identity again;
and a reference built twice from the same key is equal:

<!-- example: id-identity-compare -->
```sql
SELECT
  cassie_graph.vertex_ref('authz_a', 'm',
    ARRAY[cassie_graph.key_part(1::bigint), cassie_graph.key_part('ann'::text)])
  = cassie_graph.vertex_ref('authz_a', 'm',
    ARRAY[cassie_graph.key_part(2::bigint), cassie_graph.key_part('ann'::text)]) AS across_tenants,
  cassie_graph.vertex_ref('authz_a', 'm',
    ARRAY[cassie_graph.key_part(1::bigint), cassie_graph.key_part('ann'::text)])
  = cassie_graph.vertex_ref('authz_b', 'person',
    ARRAY[cassie_graph.key_part(1::bigint), cassie_graph.key_part('ann'::text)]) AS across_graphs,
  cassie_graph.vertex_ref('authz_a', 'm',
    ARRAY[cassie_graph.key_part(1::bigint), cassie_graph.key_part('ann'::text)])
  = cassie_graph.vertex_ref('authz_a', 'm',
    ARRAY[cassie_graph.key_part(1::bigint), cassie_graph.key_part('ann'::text)]) AS same_key
```
```text result
false | false | true
```

The self-loop (edge 3) has one vertex identity at both ends, and the parallel edges 1 and 2
share their endpoints but have distinct edge identities:

<!-- example: id-self-loop -->
```sql
SELECT s.vertex = t.vertex AS same_vertex
FROM GRAPH_TABLE (
  authz_a MATCH p = (a IS member) -[e IS grants WHERE e.id = 3]-> (b IS member)
  COLUMNS (p AS route)
) AS g
CROSS JOIN LATERAL cassie_graph.path_steps(g.route) AS s
CROSS JOIN LATERAL cassie_graph.path_steps(g.route) AS t
WHERE s.position = 0 AND t.position = 1
```
```text result
true
```

A dangling edge has no identity, because it is not an edge until both endpoints exist. Its
constructor returns the same error as a key that does not exist:

<!-- example: id-dangling-edge-ref -->
```sql
SELECT cassie_graph.edge_ref('authz_a', 'grant_edge', ARRAY[cassie_graph.key_part(5::bigint)])
```
```text sqlstate
42704
```

<!-- example: id-missing-vertex-ref -->
```sql
SELECT cassie_graph.vertex_ref('authz_a', 'm',
  ARRAY[cassie_graph.key_part(1::bigint), cassie_graph.key_part('zed'::text)])
```
```text sqlstate
42704
```

A key part of the wrong type is a different error from a key that does not exist:

<!-- example: id-wrong-key-type -->
```sql
SELECT cassie_graph.vertex_ref('authz_a', 'm',
  ARRAY[cassie_graph.key_part('1'::text), cassie_graph.key_part('ann'::text)])
```
```text sqlstate
42804
```

### A reader who may read one source only

The role `reader` has `SELECT` on `member` but not on `membership`. Every graph and every
alias that reads an edge mapping needs both tables, so neither graph, and no edge reference,
exposes the edge rows. The error is the same for an edge that exists and one that does not:

<!-- example: id-grant-schema session=root -->
```sql
GRANT USAGE ON SCHEMA public TO reader
```

<!-- example: id-grant-member session=root -->
```sql
GRANT SELECT ON TABLE public.member TO reader
```

<!-- example: id-reader-edges-a session=reader -->
```sql
SELECT g.id
FROM GRAPH_TABLE (
  authz_a MATCH (s IS member) -[e IS grants]-> (d IS member) COLUMNS (e.id AS id)
) AS g
```
```text sqlstate
42501
```

<!-- example: id-reader-edges-b session=reader -->
```sql
SELECT g.id
FROM GRAPH_TABLE (
  authz_b MATCH (s IS member) -[e IS grants]-> (d IS member) COLUMNS (e.id AS id)
) AS g
```
```text sqlstate
42501
```

<!-- example: id-reader-edge-ref session=reader -->
```sql
SELECT cassie_graph.edge_ref('authz_b', 'grants_to', ARRAY[cassie_graph.key_part(1::bigint)])
```
```text sqlstate
42501
```

<!-- example: id-reader-missing-edge-ref session=reader -->
```sql
SELECT cassie_graph.edge_ref('authz_b', 'grants_to', ARRAY[cassie_graph.key_part(404::bigint)])
```
```text sqlstate
42501
```

A path value assembled from edge references cannot be built either, because every edge
reference is checked first:

<!-- example: id-reader-path session=reader -->
```sql
SELECT s.position
FROM cassie_graph.path_steps(ROW(
  ARRAY[cassie_graph.vertex_ref('authz_b', 'person',
          ARRAY[cassie_graph.key_part(1::bigint), cassie_graph.key_part('ann'::text)]),
        cassie_graph.vertex_ref('authz_b', 'person',
          ARRAY[cassie_graph.key_part(1::bigint), cassie_graph.key_part('bob'::text)])],
  ARRAY[cassie_graph.edge_ref('authz_b', 'grants_to',
          ARRAY[cassie_graph.key_part(1::bigint)])],
  ARRAY[1::smallint])::cassie_graph.graph_path) AS s
```
```text sqlstate
42501
```

What the reader may read stays readable, through either graph:

<!-- example: id-reader-vertices session=reader -->
```sql
SELECT count(*)
FROM GRAPH_TABLE (authz_b MATCH (v IS member) COLUMNS (v.code AS code)) AS g
```
```text result
4
```

### Correcting the projection

A projector corrects membership 5 so that it points at `bob`, and creates tenant 2's missing
member `cy`. Both corrections are ordinary row changes; no graph statement is needed. Edges 5
and 7 become visible, and edge 5 gains an identity:

<!-- example: id-correct-endpoint -->
```sql
UPDATE membership SET dst = 'bob' WHERE id = 5
```

<!-- example: id-create-missing-vertex -->
```sql
INSERT INTO member VALUES (2, 'cy', 'Cy two')
```

<!-- example: id-edges-corrected -->
```sql
SELECT g.id, g.st, g.sc, g.dt, g.dc
FROM GRAPH_TABLE (
  authz_b MATCH (s IS member) -[e IS grants]-> (d IS member)
  COLUMNS (e.id AS id, s.tenant_id AS st, s.code AS sc, d.tenant_id AS dt, d.code AS dc)
) AS g
WHERE g.id IN (5, 7)
ORDER BY g.id
```
```text result
5 | 1 | ann | 1 | bob
7 | 2 | bob | 2 | cy
```

<!-- example: id-corrected-edge-ref -->
```sql
SELECT (cassie_graph.edge_ref('authz_a', 'grant_edge',
  ARRAY[cassie_graph.key_part(5::bigint)])).element_kind
```
```text result
2
```

Tenant 1 still has no `cy`, so the same display key in the other tenant stays unresolved:

<!-- example: id-other-tenant-still-missing -->
```sql
SELECT cassie_graph.vertex_ref('authz_a', 'm',
  ARRAY[cassie_graph.key_part(1::bigint), cassie_graph.key_part('cy'::text)])
```
```text sqlstate
42704
```

### Rules and error codes

| Situation | Documented behavior | SQLSTATE |
| --- | --- | --- |
| Corrections inside a transaction | The writer sees them at once; a separate session sees only the committed rows. `ROLLBACK` restores the rows seen before; `COMMIT` shows the corrected rows to the writer and to a new reader. Source-column and table renames inside the transaction follow the same rule. | none |
| Rename of a graph, a source column or a source table | Identities are unchanged: a reference saved before the rename resolves after it, and rebuilding it by the new graph name gives an equal reference. The old graph name stops resolving. | old name: `42704` |
| Delete and reinsert the same key in the same mapping | The old reference resolves to the replacement row. Identity is mapping plus typed key, with no row incarnation. | none |
| Restart of a local database | Saved references resolve unchanged. | none |
| Reference with another graph's mapping, another element kind's mapping, another database, or a dropped and recreated graph | Rejected, never retargeted. | `42704` |
| Edge or vertex that does not exist, or edge whose endpoint is missing | No identity. | `42704` |
| Wrong key type or number of key parts | Rejected. | `42804` |
| NULL graph, mapping or key array | Rejected. | `22004` |
| No `SELECT` on a mapped table | Rejected before existence is checked. | `42501` |

The `tests/graph_types/identity_*.rs` suite runs these cases against row and columnar
sources, including the restart, rename and reinsertion rows (`identity_restart.rs`) and the
writer and reader transaction runs (`identity_corrections.rs`).

## 14. Recreating graphs in a database clone

<!-- world: clone -->

A database image restores into a **new** database. The restored tables keep their rows, but
the clone holds no graph definitions, and it has its own database identity: a graph you
create in it gets fresh graph and mapping identities. Recreate the graphs from their exported
definitions.

**Export.** `information_schema.property_graphs` has a `property_graph_definition` column
with the exact `CREATE PROPERTY GRAPH` statement of each graph you may read. The statement
names the graph and its tables by schema and local name only, never by database, and lists
vertex tables before edge tables. A graph is listed only when you can read every table it
maps, so an unauthorized reader gets no definition.

<!-- example: clone-create-people -->
```sql
CREATE TABLE people (id BIGINT PRIMARY KEY, name TEXT NOT NULL)
```

<!-- example: clone-create-follows -->
```sql
CREATE TABLE follows (id BIGINT PRIMARY KEY, follower_id BIGINT NOT NULL, followee_id BIGINT NOT NULL)
```

<!-- example: clone-insert-people -->
```sql
INSERT INTO people VALUES (1, 'Ada'), (2, 'Ben'), (3, 'Cy')
```

<!-- example: clone-insert-follows -->
```sql
INSERT INTO follows VALUES (10, 1, 2), (11, 2, 3)
```

<!-- example: clone-create-graph -->
```sql
CREATE PROPERTY GRAPH social
  VERTEX TABLES (people AS person KEY (id) LABEL person PROPERTIES (id, name))
  EDGE TABLES (
    follows AS knows KEY (id)
      SOURCE KEY (follower_id) REFERENCES person (id)
      DESTINATION KEY (followee_id) REFERENCES person (id)
      LABEL knows PROPERTIES (id)
  )
```

<!-- example: clone-export -->
```sql
SELECT property_graph_definition FROM information_schema.property_graphs
WHERE property_graph_name = 'social'
```
```text result
CREATE PROPERTY GRAPH public.social VERTEX TABLES (public.people AS person KEY (id) LABEL person PROPERTIES (id, name)) EDGE TABLES (public.follows AS knows KEY (id) SOURCE KEY (follower_id) REFERENCES person (id) DESTINATION KEY (followee_id) REFERENCES person (id) LABEL knows PROPERTIES (id))
```

**Recreate.** In the clone, restore the tables first, then run each exported statement. The
graphs do not depend on one another, so their order does not matter; export them in name
order for a repeatable script. The next examples play the clone's side: the graph is dropped
to stand for a clone that has the tables but no graph, and the exported statement is run
exactly as exported.

<!-- example: clone-drop-graph -->
```sql
DROP PROPERTY GRAPH social
```

**A missing table.** `CREATE PROPERTY GRAPH` validates the whole definition and publishes it
atomically. When a mapped table has not been restored, the statement fails with `42P01` and
the clone has no partial graph: the graph stays unknown (`42704`) until a complete
statement succeeds.

<!-- example: clone-drop-follows -->
```sql
DROP TABLE follows
```

<!-- example: clone-recreate-missing-table -->
```sql
CREATE PROPERTY GRAPH public.social VERTEX TABLES (public.people AS person KEY (id) LABEL person PROPERTIES (id, name)) EDGE TABLES (public.follows AS knows KEY (id) SOURCE KEY (follower_id) REFERENCES person (id) DESTINATION KEY (followee_id) REFERENCES person (id) LABEL knows PROPERTIES (id))
```
```text sqlstate
42P01
```

<!-- example: clone-query-unpublished -->
```sql
SELECT * FROM GRAPH_TABLE (social MATCH (a IS person) COLUMNS (a.id AS id)) AS t
```
```text sqlstate
42704
```

Restore the table, then rerun the same statement unchanged:

<!-- example: clone-restore-follows -->
```sql
CREATE TABLE follows (id BIGINT PRIMARY KEY, follower_id BIGINT NOT NULL, followee_id BIGINT NOT NULL)
```

<!-- example: clone-reinsert-follows -->
```sql
INSERT INTO follows VALUES (10, 1, 2), (11, 2, 3)
```

<!-- example: clone-recreate -->
```sql
CREATE PROPERTY GRAPH public.social VERTEX TABLES (public.people AS person KEY (id) LABEL person PROPERTIES (id, name)) EDGE TABLES (public.follows AS knows KEY (id) SOURCE KEY (follower_id) REFERENCES person (id) DESTINATION KEY (followee_id) REFERENCES person (id) LABEL knows PROPERTIES (id))
```

**Rerun.** Running the statement again fails with `42710` and changes nothing: the graph, its
identities and any reference minted from them stay as they were. A recreation script that
must be rerunnable checks `information_schema.property_graphs` first and skips a graph that
is already defined.

<!-- example: clone-rerun -->
```sql
CREATE PROPERTY GRAPH public.social VERTEX TABLES (public.people AS person KEY (id) LABEL person PROPERTIES (id, name)) EDGE TABLES (public.follows AS knows KEY (id) SOURCE KEY (follower_id) REFERENCES person (id) DESTINATION KEY (followee_id) REFERENCES person (id) LABEL knows PROPERTIES (id))
```
```text sqlstate
42710
```

**Query and references.** The clone reads the same rows as its restored tables:

<!-- example: clone-query -->
```sql
SELECT * FROM GRAPH_TABLE (social MATCH (a IS person) -[IS knows]-> (b IS person) COLUMNS (a.id AS a, b.id AS b)) AS t ORDER BY a, b
```
```text result
1 | 2
2 | 3
```

A reference saved in the source database carries the **source** database, graph and mapping
identities. In the clone it is rejected with `42704`, even when the clone has a graph and
mappings with the same names; it is never retargeted. Mint a fresh reference in the clone
from the explicit typed application key instead:

<!-- example: clone-fresh-reference -->
```sql
SELECT (cassie_graph.vertex_ref('social', 'person', ARRAY[cassie_graph.key_part(2::bigint)])).element_kind
```
```text result
1
```

Two clones restored from one image get distinct identities from each other as well, so a
reference minted in one clone is also rejected in the other, and each clone must mint its
own. `tests/graph_types/clone_workflow.rs` restores real images and runs this recipe for two
graphs, two sibling clones, a missing table and a rerun; `tests/graph_types/clone_clients.rs`
runs it through `tokio-postgres` (native composite and array path) and `sqlx` (scalar and
JSON path).
