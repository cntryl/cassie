use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;

pub fn execute_statement(cassie: &Cassie, session: &CassieSession, sql: &str) {
    cassie.execute_sql(session, sql, vec![]).unwrap();
}

pub fn query_rows(cassie: &Cassie, session: &CassieSession, sql: &str) -> Vec<Vec<Value>> {
    cassie.execute_sql(session, sql, vec![]).unwrap().rows
}
