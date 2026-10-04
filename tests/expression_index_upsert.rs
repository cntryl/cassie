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
        .rows
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

#[test]
fn should_resolve_expression_index_conflicts_without_scanning_rows_per_input_row() {
    // Arrange
    let (cassie, session, path) = start("expression-index-conflict-scan");
    seed_expression_index(&cassie, &session, ROW_COUNT);
    let before = cassie.midge.query_scan_entries_for_diagnostics();
    let sql = format!(
        "INSERT INTO {TABLE} (id, email) VALUES \
         (1001, 'USER-0001@example.com'), \
         (1002, 'USER-0002@example.com'), \
         (1003, 'USER-0003@example.com'), \
         (1004, 'USER-0004@example.com') ON CONFLICT DO NOTHING"
    );

    // Act
    run(&cassie, &session, &sql);
    let scanned_entries = cassie
        .midge
        .query_scan_entries_for_diagnostics()
        .saturating_sub(before);

    // Assert
    assert!(
        scanned_entries < ROW_COUNT as u64,
        "four exact reservation lookups must not scan all {ROW_COUNT} table rows; observed {scanned_entries} scanned entries"
    );
    assert_eq!(
        rows(
            &cassie,
            &session,
            &format!("SELECT id FROM {TABLE} WHERE id >= 1000")
        ),
        [] as [Vec<Value>; 0]
    );
    cleanup(cassie, path);
}

#[test]
fn should_ignore_expression_reservation_owners_deleted_or_rekeyed_in_the_current_transaction() {
    // Arrange
    let (cassie, session, path) = start("expression-index-rekey-owner");
    run(
        &cassie,
        &session,
        &format!("CREATE UNIQUE INDEX expression_index_upsert_lower ON {TABLE} ((lower(email)))"),
    );
    run(
        &cassie,
        &session,
        &format!(
            "INSERT INTO {TABLE} (id, email) VALUES \
             (1, 'owner@example.com'), (2, 'deleted@example.com')"
        ),
    );
    run(&cassie, &session, "BEGIN");

    // Act
    run(
        &cassie,
        &session,
        &format!("UPDATE {TABLE} SET email = 'new@example.com' WHERE id = 1"),
    );
    run(
        &cassie,
        &session,
        &format!("DELETE FROM {TABLE} WHERE id = 2"),
    );
    run(
        &cassie,
        &session,
        &format!("INSERT INTO {TABLE} (id, email) VALUES (2, 'OWNER@example.com') ON CONFLICT DO NOTHING"),
    );
    run(
        &cassie,
        &session,
        &format!("INSERT INTO {TABLE} (id, email) VALUES (3, 'DELETED@example.com') ON CONFLICT DO NOTHING"),
    );
    run(&cassie, &session, "COMMIT");

    // Assert
    assert_eq!(
        rows(
            &cassie,
            &session,
            &format!("SELECT id, email FROM {TABLE} ORDER BY id")
        ),
        vec![
            vec![
                Value::Int64(1),
                Value::String("new@example.com".to_string())
            ],
            vec![
                Value::Int64(2),
                Value::String("OWNER@example.com".to_string())
            ],
            vec![
                Value::Int64(3),
                Value::String("DELETED@example.com".to_string())
            ],
        ]
    );
    cleanup(cassie, path);
}

#[test]
fn should_check_staged_expression_reservation_owner_in_the_current_transaction() {
    // Arrange
    let (cassie, session, path) = start("expression-index-staged-owner");
    run(
        &cassie,
        &session,
        &format!("CREATE UNIQUE INDEX expression_index_upsert_lower ON {TABLE} ((lower(email)))"),
    );
    run(&cassie, &session, "BEGIN");
    run(
        &cassie,
        &session,
        &format!("INSERT INTO {TABLE} (id, email) VALUES (1, 'owner@example.com')"),
    );

    // Act
    run(
        &cassie,
        &session,
        &format!("INSERT INTO {TABLE} (id, email) VALUES (2, 'OWNER@example.com') ON CONFLICT DO NOTHING"),
    );
    run(&cassie, &session, "COMMIT");

    // Assert
    assert_eq!(
        rows(&cassie, &session, &format!("SELECT id FROM {TABLE}")),
        vec![vec![Value::Int64(1)]]
    );
    cleanup(cassie, path);
}
