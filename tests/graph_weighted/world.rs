//! A small mapped graph with two edge tables, plus the equivalent oracle graph, so a
//! `graph_weighted_paths` call and the independent oracle can be compared.

use crate::graph_oracle::model::{
    vertex_ref, Edge, ElementRef, Graph, KeyPart, Scalar, Vertex, SCALE,
};
use crate::graph_oracle::weighted::{WeightedPath, WeightedQuery};
use cassie::app::{Cassie, CassieError, CassieSession};
use cassie::executor::QueryResult;
use cassie::types::Value;

/// A started in-memory instance: the reserved `cassie_graph` schema bootstraps at startup,
/// before the mapped graphs are defined.
pub struct Fixture {
    pub session: CassieSession,
    pub cassie: Cassie,
}

impl Fixture {
    /// # Errors
    ///
    /// Returns the engine error of a failing statement.
    pub fn execute(&self, sql: &str) -> Result<QueryResult, CassieError> {
        self.cassie.execute_sql(&self.session, sql, vec![])
    }

    /// # Panics
    ///
    /// Panics when the statement fails.
    pub fn rows(&self, sql: &str) -> Vec<Vec<Value>> {
        match self.execute(sql) {
            Ok(result) => result.rows().to_vec(),
            Err(error) => panic!("query failed with SQLSTATE {}", error.sql_state()),
        }
    }
}

pub fn started(config: Option<cassie::config::CassieRuntimeConfig>, setup: &[&str]) -> Fixture {
    let mut config = match config {
        Some(config) => config,
        None => cassie::config::CassieRuntimeConfig::from_env().expect("runtime config"),
    };
    if config.password.is_empty() {
        crate::support_sql::test_root_password().clone_into(&mut config.password);
    }
    let cassie = Cassie::new_in_memory_with_config(config).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    for sql in setup {
        if let Err(error) = cassie.execute_sql(&session, sql, vec![]) {
            panic!(
                "a setup statement failed with SQLSTATE {}",
                error.sql_state()
            );
        }
    }
    Fixture { session, cassie }
}

/// One edge of the `fast` (NUMERIC weight) or `slow` (INT weight) table.
#[derive(Clone, Copy)]
pub struct Link {
    pub fast: bool,
    pub id: i64,
    pub from: i64,
    pub to: i64,
    /// Weight in micro-units; `None` stores NULL.
    pub weight: Option<i128>,
}

/// A fast edge with a whole-unit weight.
pub fn fast(id: i64, from: i64, to: i64, weight: i64) -> Link {
    Link {
        fast: true,
        id,
        from,
        to,
        weight: Some(i128::from(weight) * SCALE),
    }
}

/// A slow edge with a whole-unit weight.
pub fn slow(id: i64, from: i64, to: i64, weight: i64) -> Link {
    Link {
        fast: false,
        id,
        from,
        to,
        weight: Some(i128::from(weight) * SCALE),
    }
}

/// A fast edge with a weight in micro-units.
pub fn fast_micro(id: i64, from: i64, to: i64, micro: i128) -> Link {
    Link {
        weight: Some(micro),
        ..fast(id, from, to, 0)
    }
}

/// An edge with no weight.
pub fn unweighted(id: i64, from: i64, to: i64) -> Link {
    Link {
        weight: None,
        ..fast(id, from, to, 0)
    }
}

pub struct Wg {
    pub fixture: Fixture,
    pub graph: Graph,
    vertex_mapping: u128,
}

/// One result row reduced to what the contract specifies.
#[derive(Debug, PartialEq, Eq)]
pub struct Row {
    pub rank: u64,
    pub cost: i128,
    pub edge_costs: Vec<i128>,
    pub vertices: Vec<i64>,
    pub edges: Vec<(u128, i64)>,
    pub directions: Vec<u8>,
}

fn micro_of(text: &str) -> i128 {
    let (negative, digits) = text
        .strip_prefix('-')
        .map_or((false, text), |rest| (true, rest));
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    let mut padded = fraction.to_owned();
    padded.truncate(6);
    while padded.len() < 6 {
        padded.push('0');
    }
    let value = whole.parse::<i128>().expect("whole digits") * SCALE
        + padded.parse::<i128>().expect("fraction digits");
    if negative {
        -value
    } else {
        value
    }
}

fn weight_sql(link: &Link) -> String {
    match link.weight {
        None => "NULL".to_owned(),
        Some(micro) if link.fast => {
            let sign = if micro < 0 { "-" } else { "" };
            format!("{sign}{}.{:06}", micro.abs() / SCALE, micro.abs() % SCALE)
        }
        Some(micro) => (micro / SCALE).to_string(),
    }
}

/// The vertex reference expression of vertex `id` in graph `wg`.
pub fn vertex(id: i64) -> String {
    format!("cassie_graph.vertex_ref('wg', 'n', ARRAY[cassie_graph.key_part({id}::bigint)])")
}

/// The fast-edge reference expression of edge `id`.
pub fn fast_edge(id: i64) -> String {
    format!("cassie_graph.edge_ref('wg', 'f', ARRAY[cassie_graph.key_part({id}::bigint)])")
}

impl Wg {
    /// Builds the tables, rows and `CREATE PROPERTY GRAPH wg` over vertices `1..=vertices`.
    ///
    /// # Panics
    ///
    /// Panics when a setup statement fails.
    pub fn new(vertices: i64, links: &[Link]) -> Self {
        Self::build(vertices, links, None)
    }

    /// [`Wg::new`] under explicit runtime limits.
    pub fn with_config(
        vertices: i64,
        links: &[Link],
        config: cassie::config::CassieRuntimeConfig,
    ) -> Self {
        Self::build(vertices, links, Some(config))
    }

    fn build(
        vertices: i64,
        links: &[Link],
        config: Option<cassie::config::CassieRuntimeConfig>,
    ) -> Self {
        let mut setup = vec![
            "CREATE TABLE v (id BIGINT PRIMARY KEY)".to_owned(),
            "CREATE TABLE fast (id BIGINT PRIMARY KEY, a BIGINT NOT NULL, b BIGINT NOT NULL, \
             weight NUMERIC, note TEXT, ratio DOUBLE PRECISION)"
                .to_owned(),
            "CREATE TABLE slow (id BIGINT PRIMARY KEY, a BIGINT NOT NULL, b BIGINT NOT NULL, \
             weight INT, extra INT)"
                .to_owned(),
        ];
        for id in 1..=vertices {
            setup.push(format!("INSERT INTO v VALUES ({id})"));
        }
        for link in links {
            let table = if link.fast { "fast" } else { "slow" };
            let tail = if link.fast { "'x', 1.5" } else { "1" };
            setup.push(format!(
                "INSERT INTO {table} VALUES ({}, {}, {}, {}, {tail})",
                link.id,
                link.from,
                link.to,
                weight_sql(link)
            ));
        }
        for name in ["wg", "other"] {
            setup.push(format!(
                "CREATE PROPERTY GRAPH {name} \
                 VERTEX TABLES (v AS n KEY (id) LABEL node PROPERTIES (id)) \
                 EDGE TABLES (\
                   fast AS f KEY (id) SOURCE KEY (a) REFERENCES n (id) \
                     DESTINATION KEY (b) REFERENCES n (id) \
                     LABEL fast PROPERTIES (weight, note, ratio), \
                   slow AS s KEY (id) SOURCE KEY (a) REFERENCES n (id) \
                     DESTINATION KEY (b) REFERENCES n (id) \
                     LABEL slow PROPERTIES (weight, extra))"
            ));
        }
        let statements = setup.iter().map(String::as_str).collect::<Vec<_>>();
        let fixture = started(config, &statements);
        let defined = fixture
            .cassie
            .catalog
            .list_property_graphs()
            .into_iter()
            .find(|graph| graph.name.rsplit('.').next() == Some("wg"))
            .expect("graph wg exists");
        let id = |text: &str| uuid::Uuid::parse_str(text).expect("uuid").as_u128();
        let vertex_mapping = id(&defined.vertices[0].mapping_id);
        let fast_mapping = id(&defined.edges[0].mapping_id);
        let slow_mapping = id(&defined.edges[1].mapping_id);
        let graph = Self::oracle_graph(
            vertices,
            links,
            [vertex_mapping, fast_mapping, slow_mapping],
        );
        Self {
            fixture,
            graph,
            vertex_mapping,
        }
    }

    fn oracle_graph(vertices: i64, links: &[Link], mappings: [u128; 3]) -> Graph {
        let [vertex_mapping, fast_mapping, slow_mapping] = mappings;
        let nodes = (1..=vertices)
            .map(|id| Vertex::new("n", vertex_mapping, vec![KeyPart::BigInt(id)], &["node"]))
            .collect();
        let edges = links
            .iter()
            .map(|link| {
                let from = vertex_ref(vertex_mapping, vec![KeyPart::BigInt(link.from)]);
                let to = vertex_ref(vertex_mapping, vec![KeyPart::BigInt(link.to)]);
                let (alias, mapping, label) = if link.fast {
                    ("f", fast_mapping, "fast")
                } else {
                    ("s", slow_mapping, "slow")
                };
                let weight = match (link.weight, link.fast) {
                    (None, _) => Scalar::Null,
                    (Some(micro), true) => Scalar::Num(micro),
                    (Some(micro), false) => {
                        Scalar::Int(i64::try_from(micro / SCALE).expect("small weight"))
                    }
                };
                Edge::new(
                    alias,
                    mapping,
                    vec![KeyPart::BigInt(link.id)],
                    &[label],
                    &from,
                    &to,
                )
                .prop("weight", weight)
            })
            .collect();
        Graph {
            vertices: nodes,
            edges,
        }
    }

    /// The oracle reference of vertex `id`.
    pub fn oracle_vertex(&self, id: i64) -> ElementRef {
        vertex_ref(self.vertex_mapping, vec![KeyPart::BigInt(id)])
    }

    /// The `SELECT` of a call with SQL argument text `arguments`.
    pub fn select(arguments: &str) -> String {
        format!(
            "SELECT p.path_rank, p.cost::text, p.edge_costs::text, \
             cassie_graph.path_to_jsonb(p.path)::text FROM graph_weighted_paths({arguments}) AS p \
             ORDER BY p.path_rank"
        )
    }

    /// Runs a call over vertices `from` and `to` with `rest` as its remaining arguments.
    ///
    /// # Errors
    ///
    /// The SQLSTATE of a failing call.
    pub fn call(&self, from: i64, to: i64, rest: &str) -> Result<Vec<Row>, String> {
        self.call_with(&format!("'wg', {}, {}, {rest}", vertex(from), vertex(to)))
    }

    /// Runs a call with exactly `arguments`.
    ///
    /// # Errors
    ///
    /// The SQLSTATE of a failing call.
    pub fn call_with(&self, arguments: &str) -> Result<Vec<Row>, String> {
        match self.fixture.execute(&Self::select(arguments)) {
            Ok(result) => Ok(result.rows().iter().map(|row| row_of(row)).collect()),
            Err(error) => Err(error.sql_state().to_owned()),
        }
    }

    /// The oracle's answer to the same call.
    ///
    /// # Errors
    ///
    /// The oracle's rejection, as its debug name.
    pub fn oracle(
        &self,
        from: i64,
        to: i64,
        query: impl FnOnce(&ElementRef, &ElementRef) -> OracleCall,
    ) -> Result<Vec<Row>, String> {
        let (source, target) = (self.oracle_vertex(from), self.oracle_vertex(to));
        let call = query(&source, &target);
        let labels = call
            .labels
            .as_deref()
            .map(|l| l.iter().map(String::as_str).collect::<Vec<_>>());
        let query = WeightedQuery {
            source: &source,
            target: &target,
            weight_property: "weight",
            max_hops: call.max_hops,
            direction: call.direction,
            edge_labels: labels.as_deref(),
            max_cost: call.max_cost,
            k: call.k,
        };
        crate::graph_oracle::weighted::weighted_paths(&self.graph, &query)
            .map(|paths| paths.iter().map(oracle_row).collect())
            .map_err(|error| format!("{error:?}"))
    }

    /// Statements that must succeed.
    pub fn run(&self, sql: &str) {
        if let Err(error) = self.fixture.execute(sql) {
            panic!("statement failed with SQLSTATE {}", error.sql_state());
        }
    }

    /// Runs a statement that must succeed as `root`.
    pub fn run_as_root(&self, sql: &str) {
        let root = self.fixture.cassie.create_session("root", None);
        if let Err(error) = self.fixture.cassie.execute_sql(&root, sql, vec![]) {
            panic!("statement failed with SQLSTATE {}", error.sql_state());
        }
    }
}

/// The remaining parameters of an oracle call.
pub struct OracleCall {
    pub max_hops: u32,
    pub direction: crate::graph_oracle::model::Direction,
    pub labels: Option<Vec<String>>,
    pub max_cost: Option<i128>,
    pub k: u32,
}

impl OracleCall {
    pub fn new(max_hops: u32, k: u32) -> Self {
        Self {
            max_hops,
            direction: crate::graph_oracle::model::Direction::Forward,
            labels: None,
            max_cost: None,
            k,
        }
    }
}

fn oracle_row(found: &WeightedPath) -> Row {
    Row {
        rank: found.rank,
        cost: found.cost,
        edge_costs: found.edge_costs.clone(),
        vertices: found.path.vertices.iter().map(id_of).collect(),
        edges: found
            .path
            .edges
            .iter()
            .map(|e| (e.mapping_id, id_of(e)))
            .collect(),
        directions: found.path.directions.clone(),
    }
}

fn id_of(element: &ElementRef) -> i64 {
    match element.key[0] {
        KeyPart::BigInt(number) => number,
        _ => panic!("test keys are bigint"),
    }
}

fn text(value: &Value) -> &str {
    match value {
        Value::String(text) => text,
        _ => panic!("expected text"),
    }
}

fn number(entry: &serde_json::Value) -> i64 {
    entry["key_parts"][0]["value"]
        .as_str()
        .expect("key text")
        .parse()
        .expect("integer key")
}

fn row_of(row: &[Value]) -> Row {
    let rank = match &row[0] {
        Value::Int64(rank) => u64::try_from(*rank).expect("positive rank"),
        _ => panic!("rank is an integer"),
    };
    let costs = text(&row[2]).trim_matches(|c| c == '{' || c == '}');
    let edge_costs = if costs.is_empty() {
        Vec::new()
    } else {
        costs.split(',').map(micro_of).collect()
    };
    let json: serde_json::Value = serde_json::from_str(text(&row[3])).expect("path json");
    let list = |name: &str| json[name].as_array().expect("array").clone();
    Row {
        rank,
        cost: micro_of(text(&row[1])),
        edge_costs,
        vertices: list("vertices").iter().map(number).collect(),
        edges: list("edges")
            .iter()
            .map(|edge| {
                let mapping = uuid::Uuid::parse_str(edge["mapping_id"].as_str().expect("mapping"))
                    .expect("uuid")
                    .as_u128();
                (mapping, number(edge))
            })
            .collect(),
        directions: list("directions")
            .iter()
            .map(|d| u8::try_from(d.as_i64().expect("direction")).expect("small"))
            .collect(),
    }
}
