#[test]
fn should_reject_invalid_json_string_comparands() {
    // Arrange
    use_local_storage();
    let path = data_dir("invalid_json_string_comparands");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE invalid_json_literals (id TEXT, doc JSONB)",
            vec![],
        )
        .expect("create JSONB table");

    // Act
    let results = [
        "SELECT id FROM invalid_json_literals WHERE doc = 'not-json'",
        "SELECT id FROM invalid_json_literals WHERE 'not-json' <> doc",
        "SELECT id FROM invalid_json_literals WHERE doc IN ('{}', 'not-json')",
    ]
    .map(|sql| cassie.execute_sql(&session, sql, vec![]));

    // Assert
    for result in results {
        assert!(
            matches!(result, Err(CassieError::Planner(message)) if message.contains("invalid JSON literal"))
        );
    }
    let _ = std::fs::remove_dir_all(path);
}
