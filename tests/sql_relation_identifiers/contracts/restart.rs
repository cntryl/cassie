#[test]
fn should_preserve_quoted_dotted_relation_identity_after_restart() {
    // Arrange
    support_sql::use_local_storage();
    let path = support_sql::data_dir("quoted_relation_restart");
    let cassie = cassie::app::Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, "CREATE SCHEMA triage", vec![])
        .expect("create schema");
    cassie
        .execute_sql(&session, "CREATE TABLE triage.dot (id BIGINT)", vec![])
        .expect("create schema-qualified relation");
    cassie
        .execute_sql(&session, "CREATE TABLE \"triage.dot\" (id BIGINT)", vec![])
        .expect("create quoted dotted relation");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE \"triage.dot\"\"archive \" (id BIGINT)",
            vec![],
        )
        .expect("create relation with escaped quote and trailing space");
    cassie
        .execute_sql(&session, "INSERT INTO triage.dot (id) VALUES (9)", vec![])
        .expect("insert schema-qualified relation");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO \"triage.dot\" (id) VALUES (7)",
            vec![],
        )
        .expect("insert quoted dotted relation");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO \"triage.dot\"\"archive \" (id) VALUES (5)",
            vec![],
        )
        .expect("insert relation with escaped quote and trailing space");
    drop(session);
    drop(cassie);

    let restarted = cassie::app::Cassie::new_with_data_dir(&path).expect("reopen Cassie");
    restarted.startup().expect("start reopened Cassie");
    let session = restarted.create_session("tester", None);

    // Act
    let dotted_rows = restarted
        .execute_sql(&session, "SELECT id FROM \"triage.dot\"", vec![])
        .expect("read quoted dotted relation after restart");
    let qualified_rows = restarted
        .execute_sql(&session, "SELECT id FROM triage.dot", vec![])
        .expect("read schema-qualified relation after restart");
    let escaped_rows = restarted
        .execute_sql(
            &session,
            "SELECT id FROM \"triage.dot\"\"archive \"",
            vec![],
        )
        .expect("read relation with escaped quote and trailing space after restart");

    // Assert
    assert_eq!(
        dotted_rows.rows(),
        vec![vec![cassie::types::Value::Int64(7)]]
    );
    assert_eq!(
        qualified_rows.rows(),
        vec![vec![cassie::types::Value::Int64(9)]]
    );
    assert_eq!(
        escaped_rows.rows(),
        vec![vec![cassie::types::Value::Int64(5)]]
    );
    drop(session);
    drop(restarted);
    let _ = std::fs::remove_dir_all(path);
}
