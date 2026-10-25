#[test]
fn should_preserve_enum_order_through_alterations() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("enum_value_lifecycle").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TYPE compat_mood AS ENUM ('sad', 'happy')",
                "CREATE TABLE compat_enum_rows (mood compat_mood)",
                "INSERT INTO compat_enum_rows VALUES ('sad'), ('happy')",
            ],
        )
        .await
        .expect("create enum and typed rows");

        // Act
        client
            .batch_execute("ALTER TYPE compat_mood ADD VALUE 'neutral' BEFORE 'happy'")
            .await
            .expect("add enum value before an existing value");
        client
            .batch_execute("ALTER TYPE compat_mood RENAME VALUE 'sad' TO 'down'")
            .await
            .expect("rename an enum label without changing its ordering");
        client
            .batch_execute("INSERT INTO compat_enum_rows VALUES ('neutral')")
            .await
            .expect("store the new enum label after commit");
        let values = client
            .query("SELECT mood::text FROM compat_enum_rows ORDER BY mood", &[])
            .await
            .expect("read enum values in declaration order");
        let dependent_drop = client
            .batch_execute("DROP TYPE compat_mood")
            .await
            .expect_err("a table column depends on the enum type");
        execute_statements(
            &client,
            &["DROP TABLE compat_enum_rows", "DROP TYPE compat_mood"],
        )
        .await
        .expect("remove the dependent table and enum type");
        let remaining_types: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_type WHERE typname = 'compat_mood'",
                &[],
            )
            .await
            .expect("check enum type cleanup")
            .try_get(0)
            .expect("type count");

        // Assert
        assert_eq!(
            values
                .iter()
                .map(|row| row.get::<_, String>(0))
                .collect::<Vec<_>>(),
            vec!["down", "neutral", "happy"]
        );
        assert_eq!(db_error(&dependent_drop).code().code(), "2BP01");
        assert_eq!(remaining_types, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}
