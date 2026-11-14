//! Transactional DDL keeps working once a second database exists.
//!
//! Catalog-wide metadata scans read every database family. A transactional
//! scope owns one family, so those reads must not count as a second database.

use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;

struct DataDirectory(String);

impl Drop for DataDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn open(directory: &DataDirectory) -> Cassie {
    let cassie = Cassie::new_with_data_dir(&directory.0).expect("open local Cassie");
    cassie.startup().expect("start local Cassie");
    cassie
}

fn run(cassie: &Cassie, session: &CassieSession, sql: &str) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(session, sql, vec![])
        .unwrap_or_else(|_| panic!("statement failed: {sql}"))
        .rows()
        .to_vec()
}

fn sessions(cassie: &Cassie) -> (CassieSession, CassieSession) {
    (
        cassie.create_session("tester", Some("postgres".to_string())),
        cassie.create_session("tester", Some("tenant_two".to_string())),
    )
}

fn create_in_transaction(cassie: &Cassie, session: &CassieSession, table: &str, id: i64) {
    run(cassie, session, "BEGIN");
    run(
        cassie,
        session,
        &format!("CREATE TABLE {table} (id INT PRIMARY KEY, label TEXT)"),
    );
    run(
        cassie,
        session,
        &format!("INSERT INTO {table} VALUES ({id}, 'row')"),
    );
    run(cassie, session, "COMMIT");
}

#[test]
fn should_create_tables_in_both_databases_inside_transactions_after_create_database() {
    // Arrange
    let cassie = super::super::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("startup");
    let admin = cassie.create_session("tester", Some("postgres".to_string()));
    run(
        &cassie,
        &admin,
        "CREATE TABLE before_split (id INT PRIMARY KEY)",
    );
    run(&cassie, &admin, "CREATE DATABASE tenant_two");
    let (postgres, tenant) = sessions(&cassie);

    // Act
    create_in_transaction(&cassie, &postgres, "after_split", 1);
    create_in_transaction(&cassie, &tenant, "after_split", 2);
    let postgres_rows = run(&cassie, &postgres, "SELECT id FROM after_split");
    let tenant_rows = run(&cassie, &tenant, "SELECT id FROM after_split");

    // Assert
    assert_eq!(postgres_rows, vec![vec![Value::Int64(1)]]);
    assert_eq!(tenant_rows, vec![vec![Value::Int64(2)]]);
}

#[test]
fn should_change_a_table_definition_inside_a_transaction_after_create_database() {
    // Arrange
    let cassie = super::super::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", Some("postgres".to_string()));
    run(
        &cassie,
        &session,
        "CREATE TABLE split_items (id INT PRIMARY KEY, label TEXT)",
    );
    run(&cassie, &session, "INSERT INTO split_items VALUES (1, 'a')");
    run(&cassie, &session, "CREATE DATABASE tenant_two");

    // Act
    run(&cassie, &session, "BEGIN");
    run(
        &cassie,
        &session,
        "ALTER TABLE split_items ADD COLUMN qty INT",
    );
    run(
        &cassie,
        &session,
        "CREATE INDEX split_items_label_idx ON split_items (label)",
    );
    run(
        &cassie,
        &session,
        "UPDATE split_items SET qty = 7 WHERE id = 1",
    );
    run(&cassie, &session, "COMMIT");
    let rows = run(
        &cassie,
        &session,
        "SELECT id, label, qty FROM split_items WHERE label = 'a'",
    );

    // Assert
    assert_eq!(
        rows,
        vec![vec![
            Value::Int64(1),
            Value::String("a".to_string()),
            Value::Int64(7)
        ]]
    );
}

#[test]
fn should_roll_back_a_transactional_table_while_another_database_exists() {
    // Arrange
    let cassie = super::super::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", Some("postgres".to_string()));
    run(&cassie, &session, "CREATE DATABASE tenant_two");

    // Act
    run(&cassie, &session, "BEGIN");
    run(
        &cassie,
        &session,
        "CREATE TABLE discarded (id INT PRIMARY KEY)",
    );
    run(&cassie, &session, "ROLLBACK");
    let visible = run(
        &cassie,
        &session,
        "SELECT table_name FROM information_schema.tables WHERE table_name = 'discarded'",
    );

    // Assert
    assert!(visible.is_empty(), "the rolled back table is not published");
}

#[test]
fn should_keep_transactional_ddl_working_in_both_databases_after_restart() {
    // Arrange
    let store = DataDirectory(super::super::support_sql::data_dir(
        "multi_database_transactional_ddl_restart",
    ));
    {
        let cassie = open(&store);
        let admin = cassie.create_session("tester", Some("postgres".to_string()));
        run(&cassie, &admin, "CREATE DATABASE tenant_two");
    }

    // Act
    {
        let cassie = open(&store);
        let (postgres, tenant) = sessions(&cassie);
        create_in_transaction(&cassie, &postgres, "restart_split", 1);
        create_in_transaction(&cassie, &tenant, "restart_split", 2);
    }
    let cassie = open(&store);
    let (postgres, tenant) = sessions(&cassie);
    let postgres_rows = run(&cassie, &postgres, "SELECT id FROM restart_split");
    let tenant_rows = run(&cassie, &tenant, "SELECT id FROM restart_split");

    // Assert
    assert_eq!(postgres_rows, vec![vec![Value::Int64(1)]]);
    assert_eq!(tenant_rows, vec![vec![Value::Int64(2)]]);
}
