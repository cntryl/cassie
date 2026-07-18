use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;

#[path = "support/sql.rs"]
mod support;
use support::{data_dir, with_fallback};

#[path = "graph_transaction_semantics/delete.rs"]
mod delete;
#[path = "graph_transaction_semantics/expansion.rs"]
mod expansion;
#[path = "graph_transaction_semantics/insert.rs"]
mod insert;
#[path = "graph_transaction_semantics/neighbors.rs"]
mod neighbors;
#[path = "graph_transaction_semantics/savepoint.rs"]
mod savepoint;
#[path = "graph_transaction_semantics/shortest_path.rs"]
mod shortest_path;
#[path = "graph_transaction_semantics/update.rs"]
mod update;
#[path = "graph_transaction_semantics/visibility.rs"]
mod visibility;

fn current_thread_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

fn create_graph(cassie: &Cassie, session: &CassieSession) {
    cassie
        .execute_sql(session, "CREATE GRAPH social", vec![])
        .expect("create graph");
}

fn execute(cassie: &Cassie, session: &CassieSession, sql: &str) {
    cassie
        .execute_sql(session, sql, vec![])
        .expect("execute graph statement");
}

fn neighbor_rows(cassie: &Cassie, session: &CassieSession, direction: &str) -> Vec<Vec<Value>> {
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
