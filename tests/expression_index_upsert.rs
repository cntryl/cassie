#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;

use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;

const TABLE: &str = "expression_index_upsert_rows";
const ROW_COUNT: usize = 512;

fn start(label: &str) -> (Cassie, CassieSession, String) {
    support_sql::use_local_storage();
    let path = support_sql::data_dir(label);
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            &format!("CREATE TABLE {TABLE} (id INT, email TEXT)"),
            vec![],
        )
        .expect("create table");
    (cassie, session, path)
}

fn run(cassie: &Cassie, session: &CassieSession, sql: &str) {
    cassie
        .execute_sql(session, sql, vec![])
        .unwrap_or_else(|error| panic!("statement should succeed: {sql}: {error}"));
}

fn rows(cassie: &Cassie, session: &CassieSession, sql: &str) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(session, sql, vec![])
        .expect("query rows")
        .rows()
        .to_vec()
}

fn seed_expression_index(cassie: &Cassie, session: &CassieSession, row_count: usize) {
    let documents = (0..row_count)
        .map(|index| {
            (
                Some(format!("row-{index:04}")),
                serde_json::json!({
                    "id": index,
                    "email": format!("user-{index:04}@example.com")
                }),
            )
        })
        .collect();
    cassie
        .midge
        .put_fresh_documents(TABLE, documents)
        .expect("seed table rows");
    run(
        cassie,
        session,
        &format!("CREATE UNIQUE INDEX expression_index_upsert_lower ON {TABLE} ((lower(email)))"),
    );
}

fn cleanup(cassie: Cassie, path: String) {
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}

include!("expression_index_upsert/contracts/semantics.rs");

include!("expression_index_upsert/contracts/transactions.rs");
