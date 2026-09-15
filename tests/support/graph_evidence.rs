use cassie::app::Cassie;
use cassie::types::Value;

use crate::support_sql::{data_dir, use_local_storage};

pub struct GraphComparison {
    pub native_overlay: Vec<Vec<Value>>,
    pub adjacency: Vec<Vec<Value>>,
    pub disconnected_native_overlay: Vec<Vec<Value>>,
    pub disconnected_adjacency: Vec<Vec<Value>>,
    pub overlay_fallback_reason: String,
}

pub struct SeededGraphFixture {
    seed: u64,
    reverse_insertion: bool,
}

impl SeededGraphFixture {
    pub const fn from_seed(seed: u64, reverse_insertion: bool) -> Self {
        Self {
            seed,
            reverse_insertion,
        }
    }

    pub fn compare(&self) -> GraphComparison {
        use_local_storage();
        let path = data_dir("seeded-graph-evidence");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("graph evidence runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("graph evidence Cassie");
            cassie.startup().expect("start graph evidence Cassie");
            let session = cassie.create_session("graph-evidence", None);
            execute(&cassie, &session, "CREATE GRAPH social");
            execute(&cassie, &session, "BEGIN");
            execute(
                &cassie,
                &session,
                "INSERT INTO social_nodes (node_type, node_id) VALUES ('person', 'isolated')",
            );

            let mut edges = seeded_edges(self.seed);
            if self.reverse_insertion {
                edges.reverse();
            }
            execute(
                &cassie,
                &session,
                &format!(
                    "INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES {}",
                    edges.join(", ")
                ),
            );

            let native_overlay = neighbors(&cassie, &session, "alice");
            let disconnected_native_overlay = neighbors(&cassie, &session, "isolated");
            let overlay_fallback_reason = cassie.metrics()["graph"]["last_fallback_reason"]
                .as_str()
                .expect("graph fallback reason")
                .to_owned();
            execute(&cassie, &session, "COMMIT");
            let adjacency = neighbors(&cassie, &session, "alice");
            let disconnected_adjacency = neighbors(&cassie, &session, "isolated");

            let _ = std::fs::remove_dir_all(path);
            GraphComparison {
                native_overlay,
                adjacency,
                disconnected_native_overlay,
                disconnected_adjacency,
                overlay_fallback_reason,
            }
        })
    }
}

fn execute(cassie: &Cassie, session: &cassie::app::CassieSession, sql: &str) {
    cassie
        .execute_sql(session, sql, vec![])
        .expect("execute graph evidence statement");
}

fn neighbors(
    cassie: &Cassie,
    session: &cassie::app::CassieSession,
    node_id: &str,
) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(
            session,
            &format!(
                "SELECT edge_id, node_id, cost FROM graph_neighbors('social', 'person', '{node_id}', 'both', 'knows', 100)"
            ),
            vec![],
        )
        .expect("query graph evidence neighbors")
        .rows
}

fn seeded_edges(seed: u64) -> Vec<String> {
    let mut state = seed;
    let mut rows = (0..6)
        .map(|index| {
            let mixed = next_mixed(&mut state);
            let weight = if index < 2 {
                1.0
            } else {
                f64::from(u32::try_from(mixed % 9).expect("bounded weight") + 2) / 2.0
            };
            format!(
                "('out-{index}', 'person', 'alice', 'person', 'node-{mixed:016x}', 'knows', {weight})"
            )
        })
        .collect::<Vec<_>>();
    let incoming = next_mixed(&mut state);
    rows.push(format!(
        "('incoming', 'person', 'node-{incoming:016x}', 'person', 'alice', 'knows', 1)"
    ));
    rows.push(
        "('disconnected', 'person', 'island-a', 'person', 'island-b', 'knows', 2)".to_owned(),
    );
    rows.push("('filtered', 'person', 'alice', 'person', 'ignored', 'follows', 0.5)".to_owned());
    rows
}

fn next_mixed(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut mixed = *state;
    mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    mixed ^ (mixed >> 31)
}
