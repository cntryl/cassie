use super::*;

#[test]
fn should_clear_dead_tuple_estimate_after_vacuum() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("vacuum_dead_tuple_effect").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_vacuum_effect_rows (id INT PRIMARY KEY, keep BOOLEAN)",
                "INSERT INTO compat_vacuum_effect_rows VALUES \
                   (1, FALSE), (2, FALSE), (3, TRUE), (4, TRUE)",
                "DELETE FROM compat_vacuum_effect_rows WHERE NOT keep",
            ],
        )
        .await
        .expect("create indexed rows and delete two of them");

        // Act
        client
            .batch_execute("VACUUM compat_vacuum_effect_rows")
            .await
            .expect("vacuum the table and clean up its dead tuples");
        let statistics = client
            .query_one(
                "SELECT last_vacuum IS NOT NULL, n_dead_tup \
                 FROM pg_catalog.pg_stat_user_tables \
                 WHERE schemaname = 'public' \
                   AND relname = 'compat_vacuum_effect_rows'",
                &[],
            )
            .await
            .expect("read the table's post-vacuum statistics");
        let remaining_rows = client
            .query("SELECT id FROM compat_vacuum_effect_rows ORDER BY id", &[])
            .await
            .expect("read visible rows after vacuum");

        // Assert
        assert!(statistics.get::<_, bool>(0));
        assert_eq!(statistics.get::<_, i64>(1), 0);
        assert_eq!(
            remaining_rows
                .iter()
                .map(|row| row.get::<_, i32>(0))
                .collect::<Vec<_>>(),
            vec![3, 4]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_cluster_table_by_selected_index() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("cluster_index_catalog_effect").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_cluster_rows (id INT PRIMARY KEY, cluster_key INT)",
                "INSERT INTO compat_cluster_rows VALUES (3, 20), (1, 30), (2, 10)",
                "CREATE INDEX compat_cluster_rows_key_idx ON compat_cluster_rows (cluster_key)",
            ],
        )
        .await
        .expect("create a table with rows and an index to cluster by");
        // Act
        client
            .batch_execute("CLUSTER compat_cluster_rows USING compat_cluster_rows_key_idx")
            .await
            .expect("cluster the table by its index");
        let clustered = client
            .query_one(
                "SELECT indisclustered FROM pg_catalog.pg_index \
                 WHERE indexrelid = 'compat_cluster_rows_key_idx'::regclass",
                &[],
            )
            .await
            .expect("read the index's clustered state after CLUSTER");
        let ordered_ids = client
            .query(
                "SELECT id FROM compat_cluster_rows ORDER BY cluster_key",
                &[],
            )
            .await
            .expect("read rows after CLUSTER");

        // Assert
        assert!(clustered.get::<_, bool>(0));
        assert_eq!(
            ordered_ids
                .iter()
                .map(|row| row.get::<_, i32>(0))
                .collect::<Vec<_>>(),
            vec![2, 3, 1]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_record_explicit_checkpoint_in_checkpointer_statistics() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("checkpoint_statistics_effect").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_checkpoint_rows (id INT PRIMARY KEY, payload TEXT)",
                "INSERT INTO compat_checkpoint_rows VALUES (1, 'durable checkpoint input')",
            ],
        )
        .await
        .expect("create committed data before requesting a checkpoint");
        // Act
        client
            .batch_execute("CHECKPOINT")
            .await
            .expect("request a first checkpoint");
        let previous_checkpoints: i64 = client
            .query_one(
                "SELECT num_requested FROM pg_catalog.pg_stat_checkpointer",
                &[],
            )
            .await
            .expect("read the requested-checkpoint baseline")
            .try_get(0)
            .expect("requested-checkpoint baseline");
        client
            .batch_execute("CHECKPOINT")
            .await
            .expect("request a second checkpoint");
        let requested_checkpoints: i64 = client
            .query_one(
                "SELECT num_requested FROM pg_catalog.pg_stat_checkpointer",
                &[],
            )
            .await
            .expect("read requested-checkpoint statistics")
            .try_get(0)
            .expect("requested-checkpoint count");

        // Assert
        assert!(
            requested_checkpoints > previous_checkpoints,
            "CHECKPOINT should increment pg_stat_checkpointer.num_requested from \
             {previous_checkpoints} to {requested_checkpoints}"
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_refresh_only_the_requested_column_statistics() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("analyze_column_list_effect").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_analyze_column_list_rows \
                 (id INT, left_value TEXT, right_value TEXT)",
            )
            .await
            .expect("create the table for independent column statistics");
        client
            .batch_execute(
                "INSERT INTO compat_analyze_column_list_rows VALUES \
                 (1, 'left_old', 'right_old'), (2, 'left_old', 'right_old'), \
                 (3, 'left_old', 'right_old'), (4, 'left_old', 'right_old'), \
                 (5, 'left_old', 'right_old'), (6, 'left_old', 'right_old'), \
                 (7, 'left_old', 'right_old'), (8, 'left_old', 'right_old'), \
                 (9, 'left_old', 'right_old'), (10, 'left_old', 'right_old'), \
                 (11, 'left_rare', 'right_rare')",
            )
            .await
            .expect("insert rows with distinguishing per-column distributions");

        // Act
        let analyze = client
            .batch_execute("ANALYZE compat_analyze_column_list_rows (left_value)")
            .await;
        let selected_statistics = if analyze.is_ok() {
            Some(
                client
                    .query(
                        "SELECT attname, most_common_vals::text \
                 FROM pg_catalog.pg_stats \
                 WHERE schemaname = 'public' \
                   AND tablename = 'compat_analyze_column_list_rows' \
                   AND attname IN ('left_value', 'right_value') \
                 ORDER BY attname",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };

        drop(client);
        server.shutdown(connection).await;

        // Assert
        let analyze_sqlstate = analyze
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code());
        assert!(
            analyze.is_ok(),
            "PostgreSQL 18.6 analyzes only the listed column; SQLSTATE: \
             {analyze_sqlstate:?}"
        );
        let statistics = selected_statistics
            .expect("read catalog statistics after a successful column-scoped ANALYZE")
            .expect("query pg_stats after column-scoped ANALYZE");
        assert_eq!(statistics.len(), 1);
        assert_eq!(statistics[0].get::<_, String>(0), "left_value");
        assert_eq!(statistics[0].get::<_, String>(1), "{left_old}");
    });
}
