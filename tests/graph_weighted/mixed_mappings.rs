//! Equal-cost, equal-hop paths that differ only in mapping identity follow the shared
//! element-reference order (#1258).

use crate::graph_oracle::model::{vertex_ref, Edge, Graph, KeyPart, Scalar, Vertex, SCALE};
use crate::graph_oracle::weighted::{weighted_paths, WeightedQuery};
use crate::world::started;
use cassie::types::graph::{compare_paths, Direction, ElementKind, ElementRef, Path};
use cassie::types::Value;

const SETUP: [&str; 11] = [
    "CREATE TABLE pa (id BIGINT PRIMARY KEY)",
    "CREATE TABLE pb (id BIGINT PRIMARY KEY)",
    "CREATE TABLE ab (id BIGINT PRIMARY KEY, s BIGINT NOT NULL, d BIGINT NOT NULL, weight NUMERIC)",
    "CREATE TABLE ba (id BIGINT PRIMARY KEY, s BIGINT NOT NULL, d BIGINT NOT NULL, weight NUMERIC)",
    "CREATE TABLE aa (id BIGINT PRIMARY KEY, s BIGINT NOT NULL, d BIGINT NOT NULL, weight NUMERIC)",
    "INSERT INTO pa VALUES (1), (2), (3)",
    "INSERT INTO pb VALUES (3)",
    "INSERT INTO ab VALUES (1, 1, 3, 1)",
    "INSERT INTO ba VALUES (1, 3, 2, 1)",
    "INSERT INTO aa VALUES (1, 1, 3, 1), (2, 3, 2, 1)",
    "CREATE PROPERTY GRAPH mg \
     VERTEX TABLES (pa AS a KEY (id) LABEL node PROPERTIES (id), \
                    pb AS b KEY (id) LABEL node PROPERTIES (id)) \
     EDGE TABLES (\
       ab AS x_ab KEY (id) SOURCE KEY (s) REFERENCES a (id) \
         DESTINATION KEY (d) REFERENCES b (id) LABEL link PROPERTIES (weight), \
       ba AS x_ba KEY (id) SOURCE KEY (s) REFERENCES b (id) \
         DESTINATION KEY (d) REFERENCES a (id) LABEL link PROPERTIES (weight), \
       aa AS x_aa KEY (id) SOURCE KEY (s) REFERENCES a (id) \
         DESTINATION KEY (d) REFERENCES a (id) LABEL link PROPERTIES (weight))",
];

fn reference(alias: &str, id: i64) -> String {
    format!("cassie_graph.vertex_ref('mg', '{alias}', ARRAY[cassie_graph.key_part({id}::bigint)])")
}

fn id_of(text: &str) -> u128 {
    uuid::Uuid::parse_str(text).expect("uuid").as_u128()
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
        key: vec![cassie::types::graph::KeyPart::BigInt(
            document["key_parts"][0]["value"]
                .as_str()
                .and_then(|text| text.parse().ok())
                .expect("a bigint key"),
        )],
    }
}

fn path_of(value: &Value) -> Path {
    let Value::String(text) = value else {
        panic!("path json is text");
    };
    let document: serde_json::Value = serde_json::from_str(text).expect("path json");
    let list = |name: &str| {
        document[name]
            .as_array()
            .expect("an element array")
            .iter()
            .map(element)
            .collect::<Vec<_>>()
    };
    Path {
        vertices: list("vertices"),
        edges: list("edges"),
        directions: document["directions"]
            .as_array()
            .expect("directions")
            .iter()
            .map(|_| Direction::Forward)
            .collect(),
    }
}

/// `(mapping, key)` of every vertex then edge of a path.
fn identities(path: &Path) -> Vec<(u128, i64)> {
    path.vertices
        .iter()
        .chain(&path.edges)
        .map(|element| {
            let cassie::types::graph::KeyPart::BigInt(key) = element.key[0] else {
                panic!("bigint keys");
            };
            (element.mapping_id.as_u128(), key)
        })
        .collect()
}

/// The oracle's `(mapping, key)` identities for the call, over a graph built with the
/// catalog's mapping identities.
fn oracle_identities(vertex_ids: &[u128], edge_ids: &[u128]) -> Vec<Vec<(u128, i64)>> {
    let mut graph = Graph {
        vertices: Vec::new(),
        edges: Vec::new(),
    };
    for (mapping, alias, ids) in [
        (vertex_ids[0], "a", vec![1, 2, 3]),
        (vertex_ids[1], "b", vec![3]),
    ] {
        for id in ids {
            graph.vertices.push(Vertex::new(
                alias,
                mapping,
                vec![KeyPart::BigInt(id)],
                &["node"],
            ));
        }
    }
    let edges = [
        (
            edge_ids[0],
            "x_ab",
            (vertex_ids[0], vertex_ids[1]),
            vec![(1, 1, 3)],
        ),
        (
            edge_ids[1],
            "x_ba",
            (vertex_ids[1], vertex_ids[0]),
            vec![(1, 3, 2)],
        ),
        (
            edge_ids[2],
            "x_aa",
            (vertex_ids[0], vertex_ids[0]),
            vec![(1, 1, 3), (2, 3, 2)],
        ),
    ];
    for (mapping, alias, (from_mapping, to_mapping), rows) in edges {
        for (id, from, to) in rows {
            graph.edges.push(
                Edge::new(
                    alias,
                    mapping,
                    vec![KeyPart::BigInt(id)],
                    &["link"],
                    &vertex_ref(from_mapping, vec![KeyPart::BigInt(from)]),
                    &vertex_ref(to_mapping, vec![KeyPart::BigInt(to)]),
                )
                .prop("weight", Scalar::Num(SCALE)),
            );
        }
    }
    let (source, target) = (
        vertex_ref(vertex_ids[0], vec![KeyPart::BigInt(1)]),
        vertex_ref(vertex_ids[0], vec![KeyPart::BigInt(2)]),
    );
    let query = WeightedQuery {
        source: &source,
        target: &target,
        weight_property: "weight",
        max_hops: 2,
        direction: crate::graph_oracle::model::Direction::Forward,
        edge_labels: None,
        max_cost: None,
        k: 5,
    };
    let oracle = weighted_paths(&graph, &query).expect("oracle accepts the call");
    let identities = oracle
        .iter()
        .map(|found| {
            found
                .path
                .vertices
                .iter()
                .chain(&found.path.edges)
                .map(|element| {
                    let KeyPart::BigInt(key) = element.key[0] else {
                        panic!("bigint keys");
                    };
                    (element.mapping_id, key)
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    identities
}

#[test]
pub fn should_order_mixed_mapping_ties_by_the_shared_reference_order() {
    // Arrange
    let fixture = started(None, &SETUP);
    let defined = fixture
        .cassie
        .catalog
        .list_property_graphs()
        .into_iter()
        .find(|graph| graph.name.rsplit('.').next() == Some("mg"))
        .expect("graph mg exists");
    let vertex_ids = defined
        .vertices
        .iter()
        .map(|vertex| id_of(&vertex.mapping_id))
        .collect::<Vec<_>>();
    let edge_ids = defined
        .edges
        .iter()
        .map(|edge| id_of(&edge.mapping_id))
        .collect::<Vec<_>>();
    let sql = format!(
        "SELECT p.path_rank, cassie_graph.path_to_jsonb(p.path)::text \
         FROM graph_weighted_paths('mg', {}, {}, 'weight', 2, 'out', NULL, NULL, 5) AS p \
         ORDER BY p.path_rank",
        reference("a", 1),
        reference("a", 2)
    );

    // Act
    let paths = fixture
        .rows(&sql)
        .iter()
        .map(|row| path_of(&row[1]))
        .collect::<Vec<_>>();

    // Assert
    let oracle_identities = oracle_identities(&vertex_ids, &edge_ids);
    assert_eq!(paths.len(), 2);
    assert_eq!(
        compare_paths(&paths[0], &paths[1]),
        std::cmp::Ordering::Less
    );
    assert_eq!(
        paths.iter().map(identities).collect::<Vec<_>>(),
        oracle_identities
    );
}
