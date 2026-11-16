//! A mapped `bench_social` graph over ordinary projector tables, with independent Rust oracles
//! for the friends-of-friends and weighted-route workloads. Shared by the Tier 2 and Tier 3
//! graph benchmarks.

use cassie::app::{Cassie, CassieSession};

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

const INSERT_CHUNK: i64 = 5_000;

/// `INSERT ... SELECT` statements that load `total` generated rows in chunks, so that one
/// statement never holds more rows than the standard query-memory profile allows.
#[must_use]
pub fn chunked_inserts(table: &str, columns: &str, total: i64) -> Vec<String> {
    (1..=total)
        .step_by(usize::try_from(INSERT_CHUNK).expect("chunk size"))
        .map(|first| {
            let last = (first + INSERT_CHUNK - 1).min(total);
            format!(
                "INSERT INTO {table} SELECT {columns} FROM generate_series({first}, {last}) AS g(k)"
            )
        })
        .collect()
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
    pub fn setup(&self) -> Vec<String> {
        let (people, edges) = (self.people, self.edges);
        let mut statements = vec![
            "CREATE TABLE bench_person (id BIGINT PRIMARY KEY, name TEXT)".to_owned(),
            "CREATE TABLE bench_knows (id BIGINT PRIMARY KEY, src BIGINT NOT NULL, \
             dst BIGINT NOT NULL, weight INT NOT NULL)"
                .to_owned(),
        ];
        statements.extend(chunked_inserts("bench_person", "g.k, 'person'", people));
        statements.extend(chunked_inserts(
            "bench_knows",
            &format!(
                "g.k, ((g.k - 1) % {people}) + 1, ((g.k * {MULTIPLIER}) % {people}) + 1, \
                 (g.k % {WEIGHT_CLASSES}) + 1"
            ),
            edges,
        ));
        statements.push(
            "CREATE PROPERTY GRAPH bench_social \
             VERTEX TABLES (bench_person AS person KEY (id) LABEL person PROPERTIES (id, name)) \
             EDGE TABLES (bench_knows AS knows KEY (id) \
               SOURCE KEY (src) REFERENCES person (id) \
               DESTINATION KEY (dst) REFERENCES person (id) \
               LABEL knows PROPERTIES (weight))"
                .to_owned(),
        );
        statements
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

    /// The number of walks of `minimum..=maximum` edges starting at `start`.
    #[must_use]
    pub fn walks(&self, start: i64, minimum: usize, maximum: usize) -> usize {
        let adjacency = self.adjacency();
        let mut frontier = vec![start];
        let mut total = 0;
        for length in 1..=maximum {
            let mut next = Vec::new();
            for vertex in &frontier {
                for (destination, _) in &adjacency[usize::try_from(*vertex).expect("vertex")] {
                    next.push(*destination);
                }
            }
            if length >= minimum {
                total += next.len();
            }
            frontier = next;
        }
        total
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
        let paths = simple_paths(&adjacency, source, target, max_hops);
        Route {
            source,
            target,
            max_hops,
            k,
            expected_rows: paths.min(k),
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

/// Counts simple paths of at most `max_hops` edges; parallel edges count separately.
fn simple_paths(adjacency: &[Vec<(i64, i64)>], source: i64, target: i64, max_hops: usize) -> usize {
    fn walk(
        adjacency: &[Vec<(i64, i64)>],
        at: i64,
        target: i64,
        left: usize,
        seen: &mut Vec<i64>,
    ) -> usize {
        if at == target {
            return 1;
        }
        if left == 0 {
            return 0;
        }
        let mut found = 0;
        for (next, _) in &adjacency[usize::try_from(at).expect("vertex")] {
            if !seen.contains(next) {
                seen.push(*next);
                found += walk(adjacency, *next, target, left - 1, seen);
                seen.pop();
            }
        }
        found
    }
    walk(adjacency, source, target, max_hops, &mut vec![source])
}
