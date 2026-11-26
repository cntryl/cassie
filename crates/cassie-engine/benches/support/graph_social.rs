//! A mapped `bench_social` graph over ordinary projector tables, with independent Rust oracles
//! for the friends-of-friends and weighted-route workloads. Shared by the Tier 2 and Tier 3
//! graph benchmarks.

use cassie_engine::app::{Cassie, CassieError, CassieSession};

pub const GRAPH: &str = "bench_social";
const MULTIPLIER: i64 = 7919;
const WEIGHT_CLASSES: i64 = 9;
const ROUTE_DEPTH: usize = 3;

/// Executes one fixture statement, panicking with the SQLSTATE only.
pub fn run(cassie: &Cassie, session: &CassieSession, sql: &str) {
    if let Err(error) = cassie.execute_sql(session, sql, vec![]) {
        panic!(
            "graph fixture statement failed with SQLSTATE {}",
            error.sql_state()
        );
    }
}

const BATCH_ROWS: i64 = 1_000;

/// Rows of one generated table: `row(k)` is the JSON document of row `k`, for `k` in `1..=rows`.
pub struct TableLoad {
    pub table: &'static str,
    pub rows: i64,
    pub row: Box<dyn Fn(i64) -> serde_json::Value>,
}

/// Fixture statements: tables first, then generated rows, then the graph definition.
pub struct Plan {
    pub tables: Vec<String>,
    pub loads: Vec<TableLoad>,
    pub graph: String,
}

impl Plan {
    /// Creates the tables, writes the rows in storage batches and defines the graph. Rows go
    /// through the document write path in batches, as the other Tier 3 fixtures do.
    pub fn apply(self, cassie: &Cassie, session: &CassieSession) {
        for statement in &self.tables {
            run(cassie, session, statement);
        }
        for load in &self.loads {
            let mut first = 1;
            while first <= load.rows {
                let last = (first + BATCH_ROWS - 1).min(load.rows);
                let documents = (first..=last)
                    .map(|key| (Some(key.to_string()), (load.row)(key)))
                    .collect::<Vec<_>>();
                cassie
                    .midge
                    .put_documents(load.table, documents)
                    .unwrap_or_else(|error| {
                        panic!(
                            "graph fixture rows of {} failed with SQLSTATE {}",
                            load.table,
                            CassieError::from(error).sql_state()
                        )
                    });
                first = last + 1;
            }
        }
        run(cassie, session, &self.graph);
    }
}

pub struct Social {
    people: i64,
    edges: i64,
}

/// A weighted route request and the number of rows its oracle expects.
pub struct Route {
    pub source: i64,
    pub target: i64,
    pub max_hops: usize,
    pub k: usize,
    pub expected_rows: usize,
    /// Costs of the `k` cheapest simple paths, ascending (path rank `i + 1` has cost `i`).
    pub costs: Vec<i64>,
}

impl Route {
    /// The `graph_weighted_paths` statement of this route, with named optional arguments.
    #[must_use]
    pub fn sql(&self) -> String {
        format!(
            "SELECT w.path_rank, w.cost FROM graph_weighted_paths(graph => '{GRAPH}', \
             source => {}, target => {}, weight_property => 'weight', max_hops => {}, \
             k => {}) AS w",
            vertex_ref(self.source),
            vertex_ref(self.target),
            self.max_hops,
            self.k
        )
    }
}

/// The vertex reference expression of person `id`.
#[must_use]
pub fn vertex_ref(id: i64) -> String {
    format!(
        "cassie_graph.vertex_ref('{GRAPH}', 'person', ARRAY[cassie_graph.key_part({id}::bigint)])"
    )
}

impl Social {
    #[must_use]
    pub const fn new(people: i64, edges: i64) -> Self {
        Self { people, edges }
    }

    /// Tables, rows and the graph definition.
    #[must_use]
    pub fn plan(&self) -> Plan {
        let (people, edges) = (self.people, self.edges);
        Plan {
            tables: vec![
                "CREATE TABLE bench_person (id BIGINT PRIMARY KEY, name TEXT)".to_owned(),
                "CREATE TABLE bench_knows (id BIGINT PRIMARY KEY, src BIGINT NOT NULL, \
                 dst BIGINT NOT NULL, weight INT NOT NULL)"
                    .to_owned(),
                "CREATE INDEX bench_knows_src ON bench_knows (src)".to_owned(),
                "CREATE INDEX bench_knows_dst ON bench_knows (dst)".to_owned(),
            ],
            loads: vec![
                TableLoad {
                    table: "bench_person",
                    rows: people,
                    row: Box::new(|key| serde_json::json!({ "id": key, "name": "person" })),
                },
                TableLoad {
                    table: "bench_knows",
                    rows: edges,
                    row: Box::new(move |key| {
                        serde_json::json!({
                            "id": key,
                            "src": (key - 1) % people + 1,
                            "dst": (key * MULTIPLIER) % people + 1,
                            "weight": key % WEIGHT_CLASSES + 1,
                        })
                    }),
                },
            ],
            graph: "CREATE PROPERTY GRAPH bench_social \
                    VERTEX TABLES (bench_person AS person KEY (id) LABEL person PROPERTIES (id, name)) \
                    EDGE TABLES (bench_knows AS knows KEY (id) \
                      SOURCE KEY (src) REFERENCES person (id) \
                      DESTINATION KEY (dst) REFERENCES person (id) \
                      LABEL knows PROPERTIES (weight))"
                .to_owned(),
        }
    }

    /// Out-edges by source vertex: `(destination, weight)` in edge-key order.
    #[must_use]
    pub fn adjacency(&self) -> Vec<Vec<(i64, i64)>> {
        let mut out = vec![Vec::new(); usize::try_from(self.people).expect("people") + 1];
        for key in 1..=self.edges {
            let source = (key - 1) % self.people + 1;
            let destination = (key * MULTIPLIER) % self.people + 1;
            let weight = key % WEIGHT_CLASSES + 1;
            out[usize::try_from(source).expect("source")].push((destination, weight));
        }
        out
    }

    /// The endpoint of every walk of `minimum..=maximum` edges starting at `start`.
    #[must_use]
    pub fn walk_endpoints(&self, start: i64, minimum: usize, maximum: usize) -> Vec<i64> {
        let adjacency = self.adjacency();
        let mut frontier = vec![start];
        let mut found = Vec::new();
        for length in 1..=maximum {
            frontier = frontier
                .iter()
                .flat_map(|vertex| adjacency[usize::try_from(*vertex).expect("vertex")].iter())
                .map(|(destination, _)| *destination)
                .collect();
            if length >= minimum {
                found.extend(frontier.iter().copied());
            }
        }
        found
    }

    /// The number of walks of `minimum..=maximum` edges starting at `start`.
    #[must_use]
    pub fn walks(&self, start: i64, minimum: usize, maximum: usize) -> usize {
        self.walk_endpoints(start, minimum, maximum).len()
    }

    /// A route from `source` to the first vertex that is first reached at exactly three hops,
    /// with the number of rows its simple-path oracle expects for `k` paths.
    ///
    /// # Panics
    ///
    /// Panics when no vertex is reachable at that depth.
    #[must_use]
    pub fn route(&self, source: i64, max_hops: usize, k: usize) -> Route {
        let adjacency = self.adjacency();
        let target = reachable_at_depth(&adjacency, source, ROUTE_DEPTH)
            .expect("a vertex three hops from the source");
        let mut costs = simple_path_costs(&adjacency, source, target, max_hops);
        costs.sort_unstable();
        costs.truncate(k);
        Route {
            source,
            target,
            max_hops,
            k,
            expected_rows: costs.len(),
            costs,
        }
    }
}

fn reachable_at_depth(adjacency: &[Vec<(i64, i64)>], source: i64, depth: usize) -> Option<i64> {
    let mut frontier = vec![source];
    for _ in 0..depth {
        frontier = frontier
            .iter()
            .flat_map(|vertex| adjacency[usize::try_from(*vertex).expect("vertex")].iter())
            .map(|(destination, _)| *destination)
            .collect();
    }
    frontier.into_iter().find(|vertex| *vertex != source)
}

/// The cost of every simple path of at most `max_hops` edges; parallel edges count separately.
fn simple_path_costs(
    adjacency: &[Vec<(i64, i64)>],
    source: i64,
    target: i64,
    max_hops: usize,
) -> Vec<i64> {
    fn walk(
        adjacency: &[Vec<(i64, i64)>],
        at: i64,
        target: i64,
        left: usize,
        (seen, cost): (&mut Vec<i64>, i64),
        found: &mut Vec<i64>,
    ) {
        if at == target {
            found.push(cost);
            return;
        }
        if left == 0 {
            return;
        }
        for (next, weight) in &adjacency[usize::try_from(at).expect("vertex")] {
            if !seen.contains(next) {
                seen.push(*next);
                walk(
                    adjacency,
                    *next,
                    target,
                    left - 1,
                    (seen, cost + weight),
                    found,
                );
                seen.pop();
            }
        }
    }
    let mut found = Vec::new();
    walk(
        adjacency,
        source,
        target,
        max_hops,
        (&mut vec![source], 0),
        &mut found,
    );
    found
}
