//! Builds a tiny mapped graph in SQL and the equivalent oracle graph, so a `GRAPH_TABLE`
//! query and the independent oracle can be compared as multisets.

use crate::graph_oracle::model::{
    edge_ref, vertex_ref, Direction, Edge, ElementRef, Graph, KeyPart, Path, Scalar, Vertex,
};
use crate::graph_oracle::pattern::Pattern;
use crate::graph_oracle::select::match_paths;
use crate::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::types::Value;

/// Key shape of the vertex table.
#[derive(Clone, Copy)]
pub enum Keys {
    BigInt,
    Uuid,
    Composite,
}

#[derive(Clone, Copy)]
pub enum Storage {
    Row,
    Columnar,
}

pub struct Node {
    pub num: i64,
    pub name: &'static str,
    pub score: i32,
}

pub struct Link {
    pub num: i64,
    pub from: i64,
    pub to: i64,
    pub active: bool,
    pub weight: i32,
}

pub const NODE_MAPPING: u128 = 1;
pub const LINK_MAPPING: u128 = 2;

pub fn node(num: i64) -> Node {
    Node {
        num,
        name: "n",
        score: i32::try_from(num).expect("small number"),
    }
}

pub fn link(num: i64, from: i64, to: i64) -> Link {
    Link {
        num,
        from,
        to,
        active: true,
        weight: 1,
    }
}

pub struct World {
    pub fixture: SqlFixture,
    pub graph: Graph,
    keys: Keys,
}

fn uuid_text(num: i64) -> String {
    format!("00000000-0000-0000-0000-{num:012x}")
}

impl Keys {
    fn vertex_columns(self, storage: &str) -> String {
        match self {
            Keys::BigInt => {
                format!("num BIGINT PRIMARY KEY, name TEXT, score INT) {storage}")
            }
            Keys::Uuid => {
                format!("id UUID PRIMARY KEY, num BIGINT NOT NULL, name TEXT, score INT) {storage}")
            }
            Keys::Composite => format!(
                "tenant TEXT NOT NULL, seq INT NOT NULL, num BIGINT NOT NULL, name TEXT, \
                 score INT, PRIMARY KEY (tenant, seq)) {storage}"
            ),
        }
    }

    fn endpoint_columns(self) -> &'static str {
        match self {
            Keys::BigInt => "src BIGINT NOT NULL, dst BIGINT NOT NULL",
            Keys::Uuid => "src UUID NOT NULL, dst UUID NOT NULL",
            Keys::Composite => {
                "src_tenant TEXT NOT NULL, src_seq INT NOT NULL, \
                 dst_tenant TEXT NOT NULL, dst_seq INT NOT NULL"
            }
        }
    }

    fn vertex_key(self) -> &'static str {
        match self {
            Keys::BigInt => "num",
            Keys::Uuid => "id",
            Keys::Composite => "tenant, seq",
        }
    }

    fn endpoint(self, from: bool) -> &'static str {
        match (self, from) {
            (Keys::BigInt, true) => "src",
            (Keys::BigInt, false) => "dst",
            (Keys::Uuid, true) => "src",
            (Keys::Uuid, false) => "dst",
            (Keys::Composite, true) => "src_tenant, src_seq",
            (Keys::Composite, false) => "dst_tenant, dst_seq",
        }
    }

    fn vertex_values(self, node: &Node) -> String {
        let tail = format!("{}, '{}', {}", node.num, node.name, node.score);
        match self {
            Keys::BigInt => format!("({tail})"),
            Keys::Uuid => format!("('{}', {tail})", uuid_text(node.num)),
            Keys::Composite => format!("('t', {}, {tail})", node.num),
        }
    }

    fn endpoint_values(self, from: i64, to: i64) -> String {
        match self {
            Keys::BigInt => format!("{from}, {to}"),
            Keys::Uuid => format!("'{}', '{}'", uuid_text(from), uuid_text(to)),
            Keys::Composite => format!("'t', {from}, 't', {to}"),
        }
    }

    fn key_parts(self, num: i64) -> Vec<KeyPart> {
        match self {
            Keys::BigInt => vec![KeyPart::BigInt(num)],
            Keys::Uuid => vec![KeyPart::uuid(u128::try_from(num).expect("positive"))],
            Keys::Composite => vec![
                KeyPart::text("t"),
                KeyPart::Integer(i32::try_from(num).expect("small number")),
            ],
        }
    }
}

impl World {
    /// Builds the tables, rows and `CREATE PROPERTY GRAPH g`, and the oracle graph.
    ///
    /// # Panics
    ///
    /// Panics when a setup statement fails.
    pub fn new(keys: Keys, storage: Storage, nodes: &[Node], links: &[Link]) -> Self {
        let using = match storage {
            Storage::Row => "",
            Storage::Columnar => "USING columnar",
        };
        let mut setup = vec![
            format!("CREATE TABLE node ({}", keys.vertex_columns(using)),
            format!(
                "CREATE TABLE link (num BIGINT PRIMARY KEY, {}, active BOOLEAN, weight INT) {using}",
                keys.endpoint_columns()
            ),
        ];
        for node in nodes {
            setup.push(format!(
                "INSERT INTO node VALUES {}",
                keys.vertex_values(node)
            ));
        }
        for link in links {
            setup.push(format!(
                "INSERT INTO link VALUES ({}, {}, {}, {})",
                link.num,
                keys.endpoint_values(link.from, link.to),
                link.active,
                link.weight
            ));
        }
        setup.push(format!(
            "CREATE PROPERTY GRAPH g \
             VERTEX TABLES (node AS n KEY ({key}) LABEL node PROPERTIES (num, name, score)) \
             EDGE TABLES (link AS l KEY (num) \
               SOURCE KEY ({src}) REFERENCES n ({key}) \
               DESTINATION KEY ({dst}) REFERENCES n ({key}) \
               LABEL link PROPERTIES (num, active, weight))",
            key = keys.vertex_key(),
            src = keys.endpoint(true),
            dst = keys.endpoint(false),
        ));
        let statements = setup.iter().map(String::as_str).collect::<Vec<_>>();
        let fixture = sql_fixture("graph-world", &statements);
        let graph = Self::oracle_graph(keys, nodes, links);
        Self {
            fixture,
            graph,
            keys,
        }
    }

    /// A bigint-keyed row-store world.
    pub fn simple(nodes: &[Node], links: &[Link]) -> Self {
        Self::new(Keys::BigInt, Storage::Row, nodes, links)
    }

    fn oracle_graph(keys: Keys, nodes: &[Node], links: &[Link]) -> Graph {
        let vertices = nodes
            .iter()
            .map(|node| {
                Vertex::new("n", NODE_MAPPING, keys.key_parts(node.num), &["node"])
                    .prop("num", Scalar::Int(node.num))
                    .prop("name", Scalar::Text(node.name.to_owned()))
                    .prop("score", Scalar::Int(i64::from(node.score)))
            })
            .collect();
        let edges = links
            .iter()
            .map(|link| {
                let from = vertex_ref(NODE_MAPPING, keys.key_parts(link.from));
                let to = vertex_ref(NODE_MAPPING, keys.key_parts(link.to));
                Edge::new(
                    "l",
                    LINK_MAPPING,
                    vec![KeyPart::BigInt(link.num)],
                    &["link"],
                    &from,
                    &to,
                )
                .prop("num", Scalar::Int(link.num))
                .prop("active", Scalar::Bool(link.active))
                .prop("weight", Scalar::Int(i64::from(link.weight)))
            })
            .collect();
        Graph { vertices, edges }
    }

    /// The `num` property of a vertex or edge reference.
    ///
    /// # Panics
    ///
    /// Panics for a reference the graph does not contain.
    pub fn num(&self, element: &ElementRef) -> i64 {
        let props = self
            .graph
            .vertices
            .iter()
            .find(|v| &v.id == element)
            .map(|v| &v.props)
            .or_else(|| {
                self.graph
                    .edges
                    .iter()
                    .find(|e| &e.id == element)
                    .map(|e| &e.props)
            })
            .expect("element exists");
        match props.get("num") {
            Some(Scalar::Int(num)) => *num,
            _ => panic!("every element has a num"),
        }
    }

    /// Rows of `SELECT * FROM GRAPH_TABLE (g MATCH <pattern> COLUMNS (<columns>))` as sorted
    /// integer tuples.
    ///
    /// # Panics
    ///
    /// Panics when the statement fails or returns a non-integer value.
    pub fn query(&self, pattern: &str, columns: &str) -> Vec<Vec<i64>> {
        let sql = format!("SELECT * FROM GRAPH_TABLE (g MATCH {pattern} COLUMNS ({columns})) AS t");
        let mut rows = ints(self.fixture.rows(&sql));
        rows.sort();
        rows
    }

    /// The oracle's paths for `pattern`, shaped by `shape` and sorted.
    ///
    /// # Panics
    ///
    /// Panics when the oracle rejects the pattern.
    pub fn oracle(
        &self,
        pattern: &Pattern,
        shape: impl Fn(&Self, &Path) -> Vec<i64>,
    ) -> Vec<Vec<i64>> {
        let mut rows = match_paths(&self.graph, pattern)
            .expect("oracle accepts the pattern")
            .iter()
            .map(|path| shape(self, path))
            .collect::<Vec<_>>();
        rows.sort();
        rows
    }

    /// Asserts that the executor and the oracle return the same multiset of rows.
    pub fn assert_agrees(
        &self,
        (pattern, columns): (&str, &str),
        oracle: &Pattern,
        shape: impl Fn(&Self, &Path) -> Vec<i64>,
    ) {
        let expected = self.oracle(oracle, shape);
        let actual = self.query(pattern, columns);
        assert_eq!(actual, expected, "executor and oracle disagree");
    }

    pub fn keys(&self) -> Keys {
        self.keys
    }
}

/// Converts integer result rows.
///
/// # Panics
///
/// Panics for a non-integer value.
pub fn ints(rows: Vec<Vec<Value>>) -> Vec<Vec<i64>> {
    rows.into_iter()
        .map(|row| {
            row.into_iter()
                .map(|value| match value {
                    Value::Int64(number) => number,
                    _ => panic!("expected an integer result"),
                })
                .collect()
        })
        .collect()
}

/// `[first vertex, last vertex]` numbers of a path.
pub fn ends(world: &World, path: &Path) -> Vec<i64> {
    vec![world.num(path.start()), world.num(path.end())]
}

/// Vertex numbers along a path.
pub fn vertices(world: &World, path: &Path) -> Vec<i64> {
    path.vertices.iter().map(|v| world.num(v)).collect()
}

/// Vertex then edge numbers along a path.
pub fn vertices_and_edges(world: &World, path: &Path) -> Vec<i64> {
    let mut row = vertices(world, path);
    row.extend(path.edges.iter().map(|e| world.num(e)));
    row
}

pub fn forward() -> Direction {
    Direction::Forward
}

/// Reference helper for hand-built oracle checks.
pub fn edge_key(num: i64) -> ElementRef {
    edge_ref(LINK_MAPPING, vec![KeyPart::BigInt(num)])
}
