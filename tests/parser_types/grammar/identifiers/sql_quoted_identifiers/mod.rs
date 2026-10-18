use super::support_sql as support;

use cassie::app::{Cassie, CassieSession};
use cassie::executor::QueryResult;
use cassie::sql::ast::{QueryStatement, SelectItem};
use cassie::sql::parse_statement;
use cassie::types::Value;

use support::{data_dir, use_local_storage};

fn with_users_table(label: &str, test: impl FnOnce(&Cassie, &CassieSession)) {
    use_local_storage();
    let path = data_dir(label);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE users (id BIGINT, name TEXT)",
            "INSERT INTO users (id, name) VALUES (1, 'alice')",
            "INSERT INTO users (id, name) VALUES (2, 'bob')",
        ] {
            cassie
                .execute_sql(&session, sql, vec![])
                .expect("seed users table");
        }
        test(&cassie, &session);
    });
    let _ = std::fs::remove_dir_all(path);
}

fn run(cassie: &Cassie, session: &CassieSession, sql: &str) -> QueryResult {
    match cassie.execute_sql(session, sql, vec![]) {
        Ok(result) => result,
        Err(error) => panic!("statement failed: {sql}: {error}"),
    }
}

fn string_rows(values: &[&str]) -> Vec<Vec<Value>> {
    values
        .iter()
        .map(|value| vec![Value::String((*value).to_string())])
        .collect()
}

fn create_case_distinct_fulltext_fixture(cassie: &Cassie, session: &CassieSession) {
    run(
        cassie,
        session,
        "CREATE TABLE fulltext_case_fields (id INT, \"body\" TEXT, \"Body\" TEXT)",
    );
    run(
        cassie,
        session,
        "INSERT INTO fulltext_case_fields (id, \"body\", \"Body\") VALUES \
             (1, 'amber amber', 'violet'), (2, 'violet', 'amber amber'), \
             (3, 'amber amber', 'amber amber'), \
             (4, 'amber amber amber amber', 'amber amber amber amber')",
    );
    run(
        cassie,
        session,
        "CREATE INDEX fulltext_lower_body ON fulltext_case_fields \
             USING fulltext (\"body\") WITH (boost = 1.0)",
    );
    run(
        cassie,
        session,
        "CREATE INDEX fulltext_upper_body ON fulltext_case_fields \
             USING fulltext (\"Body\") WITH (boost = 3.0)",
    );
}

fn create_case_distinct_dml_fixture(cassie: &Cassie, session: &CassieSession) {
    run(
        cassie,
        session,
        "CREATE TABLE separate_case (\"a\" INT, \"A\" INT)",
    );
    run(
        cassie,
        session,
        "INSERT INTO separate_case (\"a\", \"A\") VALUES (1, 2)",
    );
}

include!("semantics.rs");

include!("resources.rs");

include!("parameters.rs");

include!("restart.rs");
