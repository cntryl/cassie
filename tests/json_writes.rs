#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;

use cassie::app::{Cassie, CassieSession};
use cassie::sql::ast::{CopyFormat, CopyStatement};
use cassie::types::Value;
use support_sql::{data_dir, use_local_storage};

fn with_local_json_database(name: &str, test: impl FnOnce(&Cassie, &CassieSession)) {
    use_local_storage();
    let path = data_dir(name);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        test(&cassie, &session);
        drop(session);
        drop(cassie);
    });
    let _ = std::fs::remove_dir_all(path);
}

fn seed_valid_json_row(cassie: &Cassie, session: &CassieSession, table: &str) {
    cassie
        .execute_sql(
            session,
            &format!("CREATE TABLE {table} (id TEXT, doc JSON)"),
            vec![],
        )
        .expect("create table");
    cassie
        .execute_sql(
            session,
            &format!("INSERT INTO {table} (id, doc) VALUES ('valid', '{{\"a\":1}}')"),
            vec![],
        )
        .expect("insert valid JSON");
}

include!("json_writes/contracts/semantics.rs");

include!("json_writes/contracts/errors.rs");

include!("json_writes/contracts/nulls_distinguish_json_document_null_from_sql_null.rs");

include!("json_writes/contracts/restart.rs");

include!("json_writes/contracts/nulls_apply_check_constraints_to_json_null_values.rs");
