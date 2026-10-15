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
#[test]
fn should_set_only_listed_foreign_key_columns_to_null() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("foreign_key_set_null_column_subset").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_set_null_subset_parent (\
                   left_id INT, right_id INT, PRIMARY KEY (left_id, right_id))",
                "CREATE TABLE compat_set_null_subset_child (\
                   id INT PRIMARY KEY, left_id INT, right_id INT, \
                   CONSTRAINT compat_set_null_subset_fk \
                   FOREIGN KEY (left_id, right_id) \
                   REFERENCES compat_set_null_subset_parent (left_id, right_id) \
                   ON DELETE SET NULL (left_id))",
                "INSERT INTO compat_set_null_subset_parent VALUES (1, 2)",
                "INSERT INTO compat_set_null_subset_child VALUES (10, 1, 2)",
            ],
        )
        .await
        .expect("create the composite foreign key with a one-column NULL action");

        // Act
        let deleted = client
            .execute(
                "DELETE FROM compat_set_null_subset_parent WHERE left_id = 1",
                &[],
            )
            .await
            .expect("delete the referenced parent row");
        let rows = client
            .query(
                "SELECT id, left_id, right_id FROM compat_set_null_subset_child",
                &[],
            )
            .await
            .expect("read the child after the subset NULL action");

        // Assert
        assert_eq!(deleted, 1);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<_, i32>(0), 10);
        assert_eq!(rows[0].get::<_, Option<i32>>(1), None);
        assert_eq!(rows[0].get::<_, Option<i32>>(2), Some(2));

        drop(client);
        server.shutdown(connection).await;
    });
}
