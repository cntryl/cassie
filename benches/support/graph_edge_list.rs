//! Edge-list graph fixtures with pinned seeds and plain-Rust oracles: a skewed hub among
//! ordinary vertices, and a fixed neighbourhood beside disconnected components that grow.

use std::collections::HashMap;

use cassie::app::{Cassie, CassieSession};
use serde_json::json;

/// The pinned seed of the skewed fixture's ordinary edges.
pub const SEED: u64 = 0x5EED_1355;
/// Ordinary vertices of the skewed fixture, ids `2..=ORDINARY + 1`.
pub const ORDINARY: i64 = 4_000;
/// Out-degree of the hub, vertex 1.
pub const HUB_DEGREE: i64 = 2_000;
/// Out-edges of each ordinary vertex.
pub const ORDINARY_DEGREE: usize = 2;
/// An ordinary vertex with one edge into the hub.
pub const FEEDER: i64 = 3_000;
/// Fanout and depth of the fixed neighbourhood tree of the disconnected-growth fixture.
pub const TREE_FANOUT: i64 = 4;
pub const TREE_DEPTH: u32 = 3;
/// Vertices of each disconnected ring component and components at 1x growth.
pub const RING_SIZE: i64 = 50;
pub const RINGS_AT_1X: i64 = 20;

/// Directed edges `(source, destination)` over vertices `1..=vertices`; edge `i` has key `i + 1`.
pub struct EdgeList {
    pub vertices: i64,
    pub edges: Vec<(i64, i64)>,
}

fn next(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    *state >> 33
}

/// One high-degree hub, ordinary vertices with `ORDINARY_DEGREE` seeded random out-edges
/// among ordinary vertices, and one feeder edge into the hub.
#[must_use]
pub fn skewed_hub() -> EdgeList {
    let mut state = SEED;
    let mut edges = (2..=HUB_DEGREE + 1).map(|v| (1, v)).collect::<Vec<_>>();
    let span = u64::try_from(ORDINARY).expect("ordinary span");
    for source in 2..=ORDINARY + 1 {
        for _ in 0..ORDINARY_DEGREE {
            let mut target = 2 + i64::try_from(next(&mut state) % span).expect("target");
            if target == source {
                target = 2 + (target - 1) % ORDINARY;
            }
            edges.push((source, target));
        }
    }
    edges.push((FEEDER, 1));
    EdgeList {
        vertices: ORDINARY + 1,
        edges,
    }
}

/// A fanout-4, depth-3 tree rooted at vertex 1 (the queried neighbourhood) and `rings`
/// disconnected directed rings of `RING_SIZE` vertices.
#[must_use]
pub fn disconnected(rings: i64) -> EdgeList {
    let (mut tree, mut level) = (Vec::new(), vec![1_i64]);
    let mut vertices = 1;
    for _ in 0..TREE_DEPTH {
        let mut below = Vec::new();
        for parent in &level {
            for _ in 0..TREE_FANOUT {
                vertices += 1;
                tree.push((*parent, vertices));
                below.push(vertices);
            }
        }
        level = below;
    }
    let mut edges = tree;
    for ring in 0..rings {
        let first = vertices + 1 + ring * RING_SIZE;
        for offset in 0..RING_SIZE {
            edges.push((first + offset, first + (offset + 1) % RING_SIZE));
        }
    }
    EdgeList {
        vertices: vertices + rings * RING_SIZE,
        edges,
    }
}

impl EdgeList {
    pub fn sql(start: i64, min: usize, max: usize) -> String {
        format!(
            "SELECT * FROM GRAPH_TABLE (edge_list MATCH (a IS node WHERE a.id = {start}) \
             -[IS link]->{{{min},{max}}} (b IS node) COLUMNS (b.id AS id)) AS t"
        )
    }

    /// The endpoints of every walk of `min..=max` edges from `start`, one per walk.
    #[must_use]
    pub fn walk_endpoints(&self, start: i64, min: usize, max: usize) -> Vec<i64> {
        let mut out: HashMap<i64, Vec<i64>> = HashMap::new();
        for (source, destination) in &self.edges {
            out.entry(*source).or_default().push(*destination);
        }
        let (mut frontier, mut found) = (vec![start], Vec::new());
        for length in 1..=max {
            frontier = frontier
                .iter()
                .flat_map(|vertex| out.get(vertex).into_iter().flatten().copied())
                .collect();
            if length >= min {
                found.extend(frontier.iter().copied());
            }
        }
        found
    }

    /// Creates the tables, indexes, rows and graph `edge_list` (endpoint indexes included).
    ///
    /// # Panics
    ///
    /// Panics when a fixture statement or write fails.
    pub fn populate(&self, cassie: &Cassie, session: &CassieSession) {
        for sql in [
            "CREATE TABLE el_node (id BIGINT PRIMARY KEY)",
            "CREATE TABLE el_link (id BIGINT PRIMARY KEY, src BIGINT NOT NULL, dst BIGINT NOT NULL)",
            "CREATE INDEX el_link_src ON el_link (src)",
            "CREATE INDEX el_link_dst ON el_link (dst)",
        ] {
            cassie.execute_sql(session, sql, vec![]).expect("edge list DDL");
        }
        let nodes = (1..=self.vertices)
            .map(|id| (Some(id.to_string()), json!({"id": id})))
            .collect();
        cassie
            .midge
            .put_documents("el_node", nodes)
            .expect("edge list nodes");
        let links = self
            .edges
            .iter()
            .zip(1_i64..)
            .map(|((src, dst), id)| {
                (
                    Some(id.to_string()),
                    json!({"id": id, "src": src, "dst": dst}),
                )
            })
            .collect();
        cassie
            .midge
            .put_documents("el_link", links)
            .expect("edge list links");
        cassie
            .execute_sql(
                session,
                "CREATE PROPERTY GRAPH edge_list VERTEX TABLES (el_node AS n KEY (id) LABEL node \
                 PROPERTIES (id)) EDGE TABLES (el_link AS l KEY (id) SOURCE KEY (src) \
                 REFERENCES n (id) DESTINATION KEY (dst) REFERENCES n (id) LABEL link \
                 PROPERTIES (id))",
                vec![],
            )
            .expect("edge list mapping");
    }
}
