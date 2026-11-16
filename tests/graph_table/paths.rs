//! Projected path variables: `COLUMNS (p AS path)` binds `cassie_graph.graph_path` (#1256).

use std::cmp::Ordering;

use crate::graph_oracle::model::{Direction, Path as OraclePath};
use crate::graph_oracle::pattern::{EdgePattern, Pattern, Quantifier, Selector, VertexPattern};
use crate::world::{link, node, Keys, Node, Storage, World};
use cassie::types::graph::{Direction as Step, ElementKind, ElementRef, KeyPart, Path};
use cassie::types::Value;

fn nodes(count: i64) -> Vec<Node> {
    (1..=count).map(node).collect()
}

fn any_node() -> VertexPattern {
    VertexPattern::labeled(&["node"])
}

fn forward(quantifier: Quantifier) -> EdgePattern {
    EdgePattern::new(Direction::Forward)
        .labeled(&["link"])
        .quantified(quantifier)
}

fn at(number: i64) -> VertexPattern {
    any_node().when(move |vertex| {
        vertex.props.get("num") == Some(&crate::graph_oracle::model::Scalar::Int(number))
    })
}

/// The rows of `sql`; a failure reports only its SQLSTATE.
fn rows_of(world: &World, sql: &str) -> Vec<Vec<Value>> {
    match world.fixture.execute(sql) {
        Ok(result) => result.rows().to_vec(),
        Err(error) => panic!("statement failed with SQLSTATE {}", error.sql_state()),
    }
}

fn key_part(document: &serde_json::Value) -> KeyPart {
    let text = document["value"].as_str().expect("a key value string");
    match document["type"].as_str().expect("a key type") {
        "smallint" => KeyPart::SmallInt(text.parse().expect("smallint")),
        "integer" => KeyPart::Int(text.parse().expect("integer")),
        "bigint" => KeyPart::BigInt(text.parse().expect("bigint")),
        "text" => KeyPart::Text(text.to_owned()),
        "uuid" => KeyPart::Uuid(text.parse().expect("uuid")),
        _ => panic!("an unknown key type"),
    }
}

fn element(document: &serde_json::Value) -> ElementRef {
    let identity = |field: &str| {
        document[field]
            .as_str()
            .and_then(|text| text.parse().ok())
            .expect("an identity")
    };
    ElementRef {
        database_id: identity("database_id"),
        graph_id: identity("graph_id"),
        mapping_id: identity("mapping_id"),
        kind: if document["kind"] == "vertex" {
            ElementKind::Vertex
        } else {
            ElementKind::Edge
        },
        key: document["key_parts"]
            .as_array()
            .expect("key parts")
            .iter()
            .map(key_part)
            .collect(),
    }
}

/// A path from its documented version 1 JSONB export.
fn path_of(value: &Value) -> Path {
    let Value::Json(document) = value else {
        panic!("path_to_jsonb returns jsonb");
    };
    let elements = |field: &str| {
        document[field]
            .as_array()
            .expect("an element array")
            .iter()
            .map(element)
            .collect::<Vec<_>>()
    };
    Path {
        vertices: elements("vertices"),
        edges: elements("edges"),
        directions: document["directions"]
            .as_array()
            .expect("directions")
            .iter()
            .map(|step| {
                if *step == 1 {
                    Step::Forward
                } else {
                    Step::Reverse
                }
            })
            .collect(),
    }
}

/// The paths projected by `pattern`, in result order.
fn project(world: &World, pattern: &str) -> Vec<Path> {
    let sql = format!(
        "SELECT cassie_graph.path_to_jsonb(g.path) \
         FROM GRAPH_TABLE (g MATCH {pattern} COLUMNS (p AS path)) AS g"
    );
    rows_of(world, &sql)
        .iter()
        .map(|row| path_of(&row[0]))
        .collect()
}

fn number(part: &KeyPart) -> i64 {
    match part {
        KeyPart::BigInt(number) => *number,
        KeyPart::Int(number) => i64::from(*number),
        _ => panic!("a numeric key part"),
    }
}

/// Vertex numbers, edge numbers and direction values of a projected path, for a world whose
/// vertices and edges are keyed by `num` (the last key part of composite keys).
fn shape(path: &Path) -> Vec<i64> {
    let last = |element: &ElementRef| element.key.last().map(number).expect("key");
    let mut row = path.vertices.iter().map(last).collect::<Vec<_>>();
    row.extend(path.edges.iter().map(last));
    row.extend(path.directions.iter().map(|d| i64::from(d.tag())));
    row
}

fn oracle_shape(world: &World, path: &OraclePath) -> Vec<i64> {
    let mut row = crate::world::vertices_and_edges(world, path);
    row.extend(path.directions.iter().map(|d| i64::from(*d)));
    row
}

fn sorted_shapes(paths: &[Path]) -> Vec<Vec<i64>> {
    let mut shapes = paths.iter().map(shape).collect::<Vec<_>>();
    shapes.sort();
    shapes
}

#[test]
pub(crate) fn should_bind_a_projected_path_to_the_graph_path_type() {
    // Arrange
    let world = World::simple(&nodes(2), &[link(1, 1, 2)]);

    // Act
    let paths = project(&world, "p = (a IS node) -[e IS link]-> (b IS node)");

    // Assert
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0].vertices.len(), 2);
    assert_eq!(paths[0].edges.len(), 1);
    assert_eq!(paths[0].vertices[0].kind, ElementKind::Vertex);
    assert_eq!(paths[0].edges[0].kind, ElementKind::Edge);
}

#[test]
pub(crate) fn should_project_the_contract_path_example() {
    // Arrange
    let setup = [
        "CREATE TABLE people (id BIGINT PRIMARY KEY, name TEXT)",
        "CREATE TABLE follows (id BIGINT PRIMARY KEY, follower_id BIGINT NOT NULL, \
         followee_id BIGINT NOT NULL)",
        "INSERT INTO people VALUES (9007199254740993, 'a'), (2, 'b'), (3, 'c')",
        "INSERT INTO follows VALUES (10, 9007199254740993, 2), (11, 2, 3)",
        "CREATE PROPERTY GRAPH social VERTEX TABLES (people AS person KEY (id) \
           LABEL person PROPERTIES (id, name)) \
         EDGE TABLES (follows AS knows KEY (id) SOURCE KEY (follower_id) REFERENCES person (id) \
           DESTINATION KEY (followee_id) REFERENCES person (id) LABEL knows PROPERTIES (id))",
    ];
    let fixture = crate::support_sql_fixture::sql_fixture("graph-path-contract-example", &setup);
    let sql = "SELECT g.path
FROM GRAPH_TABLE (
  social MATCH p = (a IS person WHERE a.id = $1) -[e IS knows]->{0,2} (b IS person)
  COLUMNS (p AS path)
) AS g";

    // Act
    let result = fixture.cassie.execute_sql(
        &fixture.session,
        sql,
        vec![Value::Int64(9_007_199_254_740_993)],
    );

    // Assert
    let rows = match result {
        Ok(result) => result.rows().to_vec(),
        Err(error) => panic!("statement failed with SQLSTATE {}", error.sql_state()),
    };
    assert_eq!(rows.len(), 3);
    assert!(rows
        .iter()
        .all(|row| matches!(&row[0], Value::String(text) if text.starts_with('('))));
    let decoded = fixture.cassie.execute_sql(
        &fixture.session,
        &sql.replace("SELECT g.path", "SELECT cassie_graph.path_to_jsonb(g.path)"),
        vec![Value::Int64(9_007_199_254_740_993)],
    );
    let decoded = decoded
        .unwrap_or_else(|error| panic!("statement failed with SQLSTATE {}", error.sql_state()));
    let mut paths = decoded
        .rows()
        .iter()
        .map(|row| path_of(&row[0]))
        .collect::<Vec<_>>();
    paths.sort_by_key(|path| path.edges.len());
    let lengths = paths
        .iter()
        .map(|path| (path.vertices.len(), path.edges.len(), path.directions.len()))
        .collect::<Vec<_>>();
    assert_eq!(lengths, vec![(1, 0, 0), (2, 1, 1), (3, 2, 2)]);
    for path in &paths {
        assert_eq!(
            path.vertices[0].key,
            vec![KeyPart::BigInt(9_007_199_254_740_993)]
        );
        assert!(path.directions.iter().all(|step| *step == Step::Forward));
    }
}

#[test]
pub(crate) fn should_project_a_zero_hop_path_with_one_vertex_without_edges() {
    // Arrange
    let world = World::simple(&nodes(1), &[]);

    // Act
    let paths = project(&world, "p = (a IS node)");

    // Assert
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0].vertices.len(), 1);
    assert_eq!(paths[0].edges, Vec::new());
    assert_eq!(paths[0].directions, Vec::new());
}

#[test]
pub(crate) fn should_carry_the_durable_identities_of_the_graph_elements() {
    // Arrange
    let world = World::simple(&nodes(2), &[link(7, 1, 2)]);
    let identities = rows_of(
        &world,
        "SELECT property_graph_id, database_id, element_table_alias, element_table_id \
         FROM pg_catalog.pg_property_graph_identities",
    );
    let text = |value: &Value| match value {
        Value::String(text) => text.parse::<uuid::Uuid>().ok(),
        _ => None,
    };
    let mapping = |alias: &str| {
        identities
            .iter()
            .find(|row| row[2] == Value::String(alias.to_owned()))
            .and_then(|row| Some((text(&row[0])?, text(&row[1])?, text(&row[3])?)))
            .expect("an identity row")
    };

    // Act
    let paths = project(&world, "p = (a IS node) -[e IS link]-> (b IS node)");

    // Assert
    let identity =
        |element: &ElementRef| (element.graph_id, element.database_id, element.mapping_id);
    assert_eq!(identity(&paths[0].vertices[0]), mapping("n"));
    assert_eq!(identity(&paths[0].vertices[1]), mapping("n"));
    assert_eq!(identity(&paths[0].edges[0]), mapping("l"));
    assert_ne!(
        paths[0].vertices[0].mapping_id,
        paths[0].edges[0].mapping_id
    );
}

#[test]
pub(crate) fn should_carry_composite_keys_in_key_order() {
    // Arrange
    let world = World::new(Keys::Composite, Storage::Row, &nodes(2), &[link(1, 1, 2)]);

    // Act
    let paths = project(&world, "p = (a IS node) -[e IS link]-> (b IS node)");

    // Assert
    assert_eq!(
        paths[0].vertices[0].key,
        vec![KeyPart::Text("t".to_owned()), KeyPart::Int(1)]
    );
    assert_eq!(
        paths[0].vertices[1].key,
        vec![KeyPart::Text("t".to_owned()), KeyPart::Int(2)]
    );
    assert_eq!(paths[0].edges[0].key, vec![KeyPart::BigInt(1)]);
}

#[test]
pub(crate) fn should_carry_uuid_keys_as_uuid_parts() {
    // Arrange
    let world = World::new(Keys::Uuid, Storage::Row, &nodes(2), &[link(1, 1, 2)]);

    // Act
    let paths = project(&world, "p = (a IS node) -[e IS link]-> (b IS node)");

    // Assert
    let expected = |number: u128| KeyPart::Uuid(uuid::Uuid::from_u128(number));
    assert_eq!(paths[0].vertices[0].key, vec![expected(1)]);
    assert_eq!(paths[0].vertices[1].key, vec![expected(2)]);
}

#[test]
pub(crate) fn should_preserve_the_declared_width_of_integer_keys() {
    // Arrange
    let setup = [
        "CREATE TABLE small_node (k SMALLINT PRIMARY KEY)",
        "CREATE TABLE small_link (id INT PRIMARY KEY, s SMALLINT NOT NULL, d SMALLINT NOT NULL)",
        "INSERT INTO small_node VALUES (1), (2)",
        "INSERT INTO small_link VALUES (5, 1, 2)",
        "CREATE PROPERTY GRAPH tags VERTEX TABLES (small_node AS sn KEY (k) \
           LABEL sn PROPERTIES (k)) \
         EDGE TABLES (small_link AS sl KEY (id) SOURCE KEY (s) REFERENCES sn (k) \
           DESTINATION KEY (d) REFERENCES sn (k) LABEL sl PROPERTIES (id))",
    ];
    let fixture = crate::support_sql_fixture::sql_fixture("graph-path-key-tags", &setup);

    // Act
    let result = fixture.execute(
        "SELECT cassie_graph.path_to_jsonb(g.path) FROM GRAPH_TABLE (tags MATCH p = (a IS sn) -[e IS sl]-> (b IS sn) \
         COLUMNS (p AS path)) AS g",
    );

    // Assert
    let result = result
        .unwrap_or_else(|error| panic!("statement failed with SQLSTATE {}", error.sql_state()));
    let path = path_of(&result.rows()[0][0]);
    assert_eq!(path.vertices[0].key, vec![KeyPart::SmallInt(1)]);
    assert_eq!(path.edges[0].key, vec![KeyPart::Int(5)]);
}

#[test]
pub(crate) fn should_record_reverse_plus_undirected_directions_like_the_oracle_does() {
    // Arrange
    let world = World::simple(&nodes(3), &[link(1, 1, 2), link(2, 3, 2), link(3, 3, 3)]);
    let undirected = Pattern::new(any_node()).step(
        EdgePattern::new(Direction::Undirected).labeled(&["link"]),
        any_node(),
    );
    let incoming = Pattern::new(any_node()).step(
        EdgePattern::new(Direction::Backward).labeled(&["link"]),
        any_node(),
    );

    // Act
    let both = project(&world, "p = (s IS node) -[e IS link]- (t IS node)");
    let reverse = project(&world, "p = (s IS node) <-[e IS link]- (t IS node)");

    // Assert
    assert_eq!(
        sorted_shapes(&both),
        world.oracle(&undirected, oracle_shape)
    );
    assert_eq!(
        sorted_shapes(&reverse),
        world.oracle(&incoming, oracle_shape)
    );
    assert!(reverse
        .iter()
        .all(|path| path.directions.iter().all(|step| *step == Step::Reverse)));
    assert!(both
        .iter()
        .any(|path| path.directions == vec![Step::Reverse]));
    let loops = both
        .iter()
        .filter(|path| path.edges[0].key == vec![KeyPart::BigInt(3)]);
    assert_eq!(loops.count(), 1);
}

#[test]
pub(crate) fn should_match_the_oracle_for_variable_length_projected_paths() {
    // Arrange
    let world = World::simple(
        &nodes(4),
        &[link(1, 1, 2), link(2, 2, 3), link(3, 1, 3), link(4, 3, 4)],
    );
    let oracle = Pattern::new(at(1)).step(forward(Quantifier::between(0, 3)), any_node());

    // Act
    let paths = project(
        &world,
        "p = (s IS node WHERE s.num = 1) -[e IS link]->{0,3} (t IS node)",
    );

    // Assert
    assert_eq!(sorted_shapes(&paths), world.oracle(&oracle, oracle_shape));
}

#[test]
pub(crate) fn should_choose_the_tied_shortest_path_by_the_contract_total_order() {
    // Arrange
    let world = World::simple(
        &nodes(4),
        &[
            link(9, 1, 3),
            link(8, 3, 4),
            link(7, 1, 2),
            link(5, 2, 4),
            link(6, 1, 2),
        ],
    );
    let pattern = "p = (s IS node WHERE s.num = 1) -[e IS link]->{1,2} (t IS node WHERE t.num = 4)";
    let oracle = Pattern::new(at(1))
        .step(forward(Quantifier::between(1, 2)), at(4))
        .select(Selector::AnyShortest);

    // Act
    let chosen = project(&world, &format!("{pattern} KEEP ANY SHORTEST PATH"));
    let all = project(&world, pattern);

    // Assert
    let fewest = all
        .iter()
        .map(|path| path.edges.len())
        .min()
        .expect("paths");
    let tied = all.iter().filter(|path| path.edges.len() == fewest);
    let lowest = tied.min().expect("a tied path").clone();
    assert!(all.iter().filter(|path| path.edges.len() == fewest).count() > 1);
    assert_eq!(chosen, vec![lowest]);
    assert_eq!(sorted_shapes(&chosen), world.oracle(&oracle, oracle_shape));
}

#[test]
pub(crate) fn should_choose_the_k_lowest_tied_paths_by_the_contract_total_order() {
    // Arrange
    let world = World::simple(
        &nodes(3),
        &[link(4, 1, 2), link(2, 1, 2), link(3, 1, 2), link(1, 2, 3)],
    );
    let pattern = "p = (s IS node WHERE s.num = 1) -[e IS link]->{1,1} (t IS node WHERE t.num = 2)";

    // Act
    let chosen = project(&world, &format!("{pattern} KEEP SHORTEST 2 PATHS"));
    let mut all = project(&world, pattern);
    all.sort();

    // Assert
    let mut chosen = chosen;
    chosen.sort();
    assert_eq!(chosen, all[..2].to_vec());
    assert_eq!(
        chosen
            .iter()
            .map(|path| path.edges[0].key.clone())
            .collect::<Vec<_>>(),
        vec![vec![KeyPart::BigInt(2)], vec![KeyPart::BigInt(3)]]
    );
    assert_eq!(chosen[0].cmp(&chosen[1]), Ordering::Less);
}

#[test]
pub(crate) fn should_export_a_projected_path_to_jsonb_without_rounding_bigint_keys() {
    // Arrange
    let nodes = [
        Node {
            num: 9_007_199_254_740_993,
            name: "n",
            score: 1,
        },
        node(2),
    ];
    let world = World::simple(&nodes, &[link(1, 9_007_199_254_740_993, 2)]);

    // Act
    let rows = rows_of(
        &world,
        "SELECT cassie_graph.path_to_jsonb(g.path) FROM GRAPH_TABLE (g MATCH \
         p = (a IS node) -[e IS link]-> (b IS node) COLUMNS (p AS path)) AS g",
    );

    // Assert
    let Value::Json(document) = &rows[0][0] else {
        panic!("path_to_jsonb returns jsonb");
    };
    assert_eq!(document["version"], 1);
    assert_eq!(document["directions"], serde_json::json!([1]));
    assert_eq!(
        document["vertices"][0]["key_parts"][0],
        serde_json::json!({"type": "bigint", "value": "9007199254740993"})
    );
}

#[test]
pub(crate) fn should_list_the_steps_of_a_projected_path() {
    // Arrange
    let world = World::simple(&nodes(3), &[link(1, 1, 2), link(2, 2, 3)]);

    // Act
    let rows = rows_of(
        &world,
        "SELECT s.position, s.direction, s.edge IS NULL FROM GRAPH_TABLE (g MATCH \
         p = (a IS node WHERE a.num = 1) -[e IS link]->{2} (b IS node) COLUMNS (p AS path)) AS g \
         CROSS JOIN LATERAL cassie_graph.path_steps(g.path) AS s ORDER BY s.position",
    );

    // Assert
    assert_eq!(
        rows,
        vec![
            vec![Value::Int64(0), Value::Null, Value::Bool(true)],
            vec![Value::Int64(1), Value::Int64(1), Value::Bool(false)],
            vec![Value::Int64(2), Value::Int64(1), Value::Bool(false)],
        ]
    );
}

#[test]
pub(crate) fn should_read_back_a_projected_path_stored_in_a_graph_path_column() {
    // Arrange
    let world = World::simple(&nodes(2), &[link(1, 1, 2)]);
    rows_of(
        &world,
        "CREATE TABLE kept (id INT PRIMARY KEY, route cassie_graph.graph_path)",
    );
    let pattern = "p = (a IS node) -[e IS link]-> (b IS node)";

    // Act
    rows_of(
        &world,
        &format!(
            "INSERT INTO kept SELECT 1, g.path FROM GRAPH_TABLE (g MATCH {pattern} \
             COLUMNS (p AS path)) AS g"
        ),
    );
    let stored = rows_of(&world, "SELECT cassie_graph.path_to_jsonb(route) FROM kept");
    let projected = rows_of(
        &world,
        &format!("SELECT g.path FROM GRAPH_TABLE (g MATCH {pattern} COLUMNS (p AS path)) AS g"),
    );
    let kept = rows_of(&world, "SELECT route FROM kept");

    // Assert
    assert_eq!(stored.len(), 1);
    assert_eq!(vec![path_of(&stored[0][0])], project(&world, pattern));
    assert_eq!(kept, projected);
}

#[test]
pub(crate) fn should_accept_a_projected_path_back_as_a_parameter() {
    // Arrange
    let world = World::simple(&nodes(2), &[link(1, 1, 2)]);
    let projected = rows_of(
        &world,
        "SELECT g.path FROM GRAPH_TABLE (g MATCH p = (a IS node) -[e IS link]-> (b IS node) \
         COLUMNS (p AS path)) AS g",
    );
    let parameter = projected[0][0].clone();

    // Act
    let result = world.fixture.cassie.execute_sql(
        &world.fixture.session,
        "SELECT cassie_graph.path_to_jsonb($1::cassie_graph.graph_path)",
        vec![parameter],
    );

    // Assert
    let rows = match result {
        Ok(result) => result.rows().to_vec(),
        Err(error) => panic!("statement failed with SQLSTATE {}", error.sql_state()),
    };
    let Value::Json(document) = &rows[0][0] else {
        panic!("path_to_jsonb returns jsonb");
    };
    assert_eq!(document["directions"], serde_json::json!([1]));
}

#[test]
pub(crate) fn should_resolve_a_reference_constructor_like_the_pattern_source() {
    // Arrange
    let world = crate::world::World::simple(
        &[crate::world::node(1), crate::world::node(2)],
        &[crate::world::link(1, 1, 2)],
    );
    let resolves = |sql: &str| world.fixture.execute(sql).is_ok();

    // Act
    let outcomes = ["g", "public.g", "absent"].map(|name| {
        let pattern = resolves(&format!(
            "SELECT * FROM GRAPH_TABLE ({name} MATCH (a IS node) COLUMNS (a.num AS n)) AS t"
        ));
        let vertex = resolves(&format!(
            "SELECT cassie_graph.vertex_ref('{name}', 'n', ARRAY[cassie_graph.key_part(1::bigint)])"
        ));
        let edge = resolves(&format!(
            "SELECT cassie_graph.edge_ref('{name}', 'l', ARRAY[cassie_graph.key_part(1::bigint)])"
        ));
        (pattern, vertex, edge)
    });
    let label_is_not_an_alias = world
        .fixture
        .error(
            "SELECT cassie_graph.vertex_ref('g', 'node', ARRAY[cassie_graph.key_part(1::bigint)])",
        )
        .sql_state()
        .to_owned();

    // Assert
    assert_eq!(outcomes[0], (true, true, true));
    assert_eq!(outcomes[1].1, outcomes[1].0);
    assert_eq!(outcomes[1].2, outcomes[1].0);
    assert_eq!(outcomes[2], (false, false, false));
    assert_eq!(label_is_not_an_alias, "42704");
}
