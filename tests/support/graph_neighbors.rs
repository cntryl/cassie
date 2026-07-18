use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;

pub fn neighbor_rows(cassie: &Cassie, session: &CassieSession, direction: &str) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(
            session,
            &format!(
                "SELECT node_id, cost FROM graph_neighbors('social', 'person', 'alice', '{direction}', 'knows', 10)"
            ),
            vec![],
        )
        .expect("read graph neighbors")
        .rows
}
