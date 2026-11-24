#[test]
fn should_preserve_json_null_distinction_after_restart() {
    // Arrange
    use_local_storage();
    let path = data_dir("json_null_restart");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE json_null_restart (id TEXT PRIMARY KEY, doc JSON)",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO json_null_restart (id, doc) VALUES ('json-null', 'null'), ('sql-null', NULL)",
                vec![],
            )
            .expect("insert both null values");
        drop(session);
        drop(cassie);

        // Act
        let reopened = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
        reopened.startup().expect("restart Cassie");
        let reopened_session = reopened.create_session("tester", None);
        let rows = reopened
            .execute_sql(
                &reopened_session,
                "SELECT id, doc, doc IS NULL FROM json_null_restart ORDER BY id",
                vec![],
            )
            .expect("read values after restart");

        // Assert
        assert_eq!(
            rows.rows(),
            vec![
                vec![Value::String("json-null".to_string()), Value::Json(serde_json::Value::Null), Value::Bool(false)],
                vec![Value::String("sql-null".to_string()), Value::Null, Value::Bool(true)],
            ]
        );
        drop(reopened_session);
        drop(reopened);
        let _ = std::fs::remove_dir_all(path);
    });
}
