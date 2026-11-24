//! Deterministic rooted trees: independently vary degree, depth or fanout.

use cassie::app::{Cassie, CassieSession};
use serde_json::json;

pub struct Tree {
    pub fanout: usize,
    pub depth: usize,
    pub nodes: usize,
    pub leaves: usize,
}

impl Tree {
    pub fn new(fanout: usize, depth: usize) -> Self {
        assert!(fanout > 0 && depth > 0);
        let mut nodes = 1;
        let mut leaves = 1;
        for _ in 0..depth {
            leaves *= fanout;
            nodes += leaves;
        }
        Self {
            fanout,
            depth,
            nodes,
            leaves,
        }
    }

    pub fn mapped_rows(&self) -> usize {
        self.nodes * 2 - 1
    }

    /// The `(source, destination)` of every edge: vertex `v > 1` hangs under `(v - 2) / fanout + 1`.
    pub fn edges(&self) -> Vec<(i64, i64)> {
        (2..=self.nodes)
            .map(|target| {
                let parent = (target - 2) / self.fanout + 1;
                (
                    i64::try_from(parent).expect("parent"),
                    i64::try_from(target).expect("target"),
                )
            })
            .collect()
    }

    pub fn sql(&self) -> String {
        format!(
            "SELECT * FROM GRAPH_TABLE (scaling MATCH (a IS node WHERE a.id = 1) \
            -[IS link]->{{{depth},{depth}}} (b IS node) COLUMNS (b.id AS id)) AS t",
            depth = self.depth
        )
    }

    pub fn populate(&self, cassie: &Cassie, session: &CassieSession) {
        for sql in [
            "CREATE TABLE scaling_node (id BIGINT PRIMARY KEY)",
            "CREATE TABLE scaling_link (id BIGINT PRIMARY KEY, src BIGINT NOT NULL, dst BIGINT NOT NULL)",
            "CREATE INDEX scaling_link_src ON scaling_link (src)",
            "CREATE INDEX scaling_link_dst ON scaling_link (dst)",
        ] {
            cassie.execute_sql(session, sql, vec![]).expect("scaling DDL");
        }
        let nodes = (1..=self.nodes)
            .map(|id| (Some(id.to_string()), json!({"id": id})))
            .collect();
        cassie
            .midge
            .put_documents("scaling_node", nodes)
            .expect("scaling nodes");
        let edges = (2..=self.nodes)
            .map(|target| {
                let parent = (target - 2) / self.fanout + 1;
                (
                    Some((target - 1).to_string()),
                    json!({"id": target - 1, "src": parent, "dst": target}),
                )
            })
            .collect();
        cassie
            .midge
            .put_documents("scaling_link", edges)
            .expect("scaling edges");
        cassie.execute_sql(session,
            "CREATE PROPERTY GRAPH scaling VERTEX TABLES (scaling_node AS n KEY (id) LABEL node PROPERTIES (id)) \
             EDGE TABLES (scaling_link AS l KEY (id) SOURCE KEY (src) REFERENCES n (id) \
             DESTINATION KEY (dst) REFERENCES n (id) LABEL link PROPERTIES (id))", vec![]).expect("scaling mapping");
    }
}
