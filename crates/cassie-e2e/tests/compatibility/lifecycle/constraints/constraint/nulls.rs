#[test]
fn should_allow_duplicate_null_keys_under_distinct_policy() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("unique_nulls_distinct_forms").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_unique_column_nulls_distinct (\
                   value INT CONSTRAINT compat_unique_column_nulls_distinct_key \
                   UNIQUE NULLS DISTINCT)",
                "CREATE TABLE compat_unique_table_nulls_distinct (value INT, \
                   CONSTRAINT compat_unique_table_nulls_distinct_key \
                   UNIQUE NULLS DISTINCT (value))",
                "INSERT INTO compat_unique_column_nulls_distinct VALUES (NULL), (NULL), (NULL)",
                "INSERT INTO compat_unique_table_nulls_distinct VALUES (NULL), (NULL), (NULL)",
            ],
        )
        .await
        .expect("create both explicit NULLS DISTINCT forms and insert repeated NULL keys");

        // Act
        let counts = client
            .query_one(
                "SELECT (SELECT count(*) FROM compat_unique_column_nulls_distinct), \
                        (SELECT count(*) FROM compat_unique_table_nulls_distinct)",
                &[],
            )
            .await
            .expect("count accepted NULL keys in both constraint forms");
        let catalog = client
            .query(
                "SELECT constraint_row.conname, index_row.indnullsnotdistinct \
                 FROM pg_catalog.pg_constraint AS constraint_row \
                 JOIN pg_catalog.pg_index AS index_row \
                   ON index_row.indexrelid = constraint_row.conindid \
                 WHERE constraint_row.conname IN (\
                   'compat_unique_column_nulls_distinct_key', \
                   'compat_unique_table_nulls_distinct_key') \
                 ORDER BY constraint_row.conname",
                &[],
            )
            .await
            .expect("inspect null-distinct policy for both named constraints");

        // Assert
        assert_eq!(counts.get::<_, i64>(0), 3);
        assert_eq!(counts.get::<_, i64>(1), 3);
        assert_eq!(catalog.len(), 2);
        assert_eq!(
            catalog[0].get::<_, String>(0),
            "compat_unique_column_nulls_distinct_key"
        );
        assert!(!catalog[0].get::<_, bool>(1));
        assert_eq!(
            catalog[1].get::<_, String>(0),
            "compat_unique_table_nulls_distinct_key"
        );
        assert!(!catalog[1].get::<_, bool>(1));

        drop(client);
        server.shutdown(connection).await;
    });
}
