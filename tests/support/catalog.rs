use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;
use std::path::PathBuf;
use uuid::Uuid;

pub fn with_fallback() {
    if std::env::var("CASSIE_EMBEDDINGS_PROVIDER").is_err() {
        std::env::set_var("CASSIE_EMBEDDINGS_PROVIDER", "fallback");
    }
}

pub fn data_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("cassie-catalog-{name}-{}", Uuid::new_v4()))
}

pub fn execute_statement(cassie: &Cassie, session: &CassieSession, sql: &str) {
    cassie.execute_sql(session, sql, vec![]).unwrap();
}

pub fn query_rows(cassie: &Cassie, session: &CassieSession, sql: &str) -> Vec<Vec<Value>> {
    cassie.execute_sql(session, sql, vec![]).unwrap().rows
}
