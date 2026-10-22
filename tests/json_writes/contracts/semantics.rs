#[test]
fn should_normalize_json_text_writes() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE json_text_writes (id TEXT PRIMARY KEY, doc JSON)",
                vec![],
            )
            .expect("create table");

        // Act
        cassie
            .execute_sql(
                &session,
                "INSERT INTO json_text_writes (id, doc) VALUES ('insert', '{\"a\":1}')",
                vec![],
            )
            .expect("insert JSON text");
        cassie
            .copy_from_csv_stdin(
                &session,
                &CopyStatement {
                    table: "json_text_writes".to_string(),
                    columns: vec!["id".to_string(), "doc".to_string()],
                    format: CopyFormat::Csv,
                    header: false,
                },
                b"copy,\"{\"\"a\"\":1}\"\n",
            )
            .expect("copy JSON text");
        let distinct = cassie
            .execute_sql(
                &session,
                "SELECT DISTINCT doc FROM json_text_writes",
                vec![],
            )
            .expect("select distinct JSON value");
        cassie
            .execute_sql(
                &session,
                "UPDATE json_text_writes SET doc = '{\"k\":2}' WHERE id = 'insert'",
                vec![],
            )
            .expect("update JSON text");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO json_text_writes (id, doc) VALUES ('insert', '{\"r\":3}') ON CONFLICT (id) DO UPDATE SET doc = '{\"c\":3}'",
                vec![],
            )
            .expect("upsert JSON text");
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT doc FROM json_text_writes WHERE id = 'insert'",
                vec![],
            )
            .expect("select JSON value");

        // Assert
        assert_eq!(
            distinct.rows(),
            vec![vec![Value::Json(serde_json::json!({"a": 1}))]]
        );
        assert_eq!(
            selected.rows(),
            vec![vec![Value::Json(serde_json::json!({"c": 3}))]]
        );

    });
}
