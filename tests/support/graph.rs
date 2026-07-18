pub(crate) use cassie::app::Cassie;
use cassie::app::CassieSession;
pub(crate) use cassie::types::Value;

#[path = "sql.rs"]
mod sql;
pub(crate) use sql::{data_dir, with_fallback};

pub(crate) fn current_thread_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

pub(crate) fn create_graph(cassie: &Cassie, session: &CassieSession) {
    cassie
        .execute_sql(session, "CREATE GRAPH social", vec![])
        .expect("create graph");
}

pub(crate) fn execute(cassie: &Cassie, session: &CassieSession, sql: &str) {
    cassie
        .execute_sql(session, sql, vec![])
        .expect("execute graph statement");
}
