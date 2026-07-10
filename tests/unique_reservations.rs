use cassie::app::Cassie;
use cassie::types::Value;

#[path = "support/sql.rs"]
mod support;

#[test]
fn should_release_unique_reservation_when_document_is_deleted() {
    // Arrange
    support::with_fallback();
    let path = support::data_dir("unique_reservation_delete");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE unique_reservation_delete (email TEXT UNIQUE)",
            vec![],
        )
        .expect("create table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO unique_reservation_delete (email) VALUES ('reuse@example.com')",
            vec![],
        )
        .expect("insert original row");

    // Act
    cassie
        .execute_sql(
            &session,
            "DELETE FROM unique_reservation_delete WHERE email = 'reuse@example.com'",
            vec![],
        )
        .expect("delete original row");
    let inserted = cassie
        .execute_sql(
            &session,
            "INSERT INTO unique_reservation_delete (email) VALUES ('reuse@example.com')",
            vec![],
        )
        .expect("reuse unique value");
    let rows = cassie
        .execute_sql(
            &session,
            "SELECT email FROM unique_reservation_delete",
            vec![],
        )
        .expect("select rows");

    // Assert
    assert_eq!(inserted.command, "INSERT 0 1");
    assert_eq!(
        rows.rows,
        vec![vec![Value::String("reuse@example.com".to_string())]]
    );

    let _ = std::fs::remove_dir_all(path);
}
