#[test]
fn should_report_merge_action_values_through_tokio_postgres() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("merge_action_returning").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_merge_target (id INT PRIMARY KEY, label TEXT)",
                "CREATE TABLE compat_merge_source (id INT PRIMARY KEY, label TEXT)",
                "INSERT INTO compat_merge_target VALUES (1, 'old')",
                "INSERT INTO compat_merge_source VALUES (1, 'updated'), (2, 'inserted')",
            ],
        )
        .await
        .expect("create merge fixtures");

        // Act
        let actions = client
            .query(
                "MERGE INTO compat_merge_target AS target \
                 USING compat_merge_source AS source ON target.id = source.id \
                 WHEN MATCHED THEN UPDATE SET label = source.label \
                 WHEN NOT MATCHED THEN INSERT (id, label) VALUES (source.id, source.label) \
                 RETURNING merge_action(), target.id, target.label",
                &[],
            )
            .await
            .expect("execute MERGE and return action values");
        let mut observed = actions
            .iter()
            .map(|row| {
                (
                    row.get::<_, String>(0),
                    row.get::<_, i32>(1),
                    row.get::<_, String>(2),
                )
            })
            .collect::<Vec<_>>();
        observed.sort_by_key(|row| row.1);

        // Assert
        assert_eq!(
            observed,
            vec![
                ("UPDATE".to_string(), 1, "updated".to_string()),
                ("INSERT".to_string(), 2, "inserted".to_string())
            ]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_delete_merge_targets_missing_from_the_source() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("merge_not_matched_by_source").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_merge_by_source_target (id INT PRIMARY KEY, label TEXT)",
                "CREATE TABLE compat_merge_by_source_source (id INT, label TEXT)",
                "INSERT INTO compat_merge_by_source_target VALUES (1, 'old'), (2, 'remove')",
                "INSERT INTO compat_merge_by_source_source VALUES (1, 'updated')",
            ],
        )
        .await
        .expect("create MERGE BY SOURCE fixtures");

        // Act
        let actions = client
            .query(
                "MERGE INTO compat_merge_by_source_target AS target \
                 USING compat_merge_by_source_source AS source ON target.id = source.id \
                 WHEN MATCHED THEN UPDATE SET label = source.label \
                 WHEN NOT MATCHED BY SOURCE THEN DELETE \
                 RETURNING merge_action(), target.id",
                &[],
            )
            .await
            .expect("update matched rows and delete target-only rows");
        let mut observed = actions
            .iter()
            .map(|row| (row.get::<_, String>(0), row.get::<_, i32>(1)))
            .collect::<Vec<_>>();
        observed.sort_by_key(|row| row.1);

        // Assert
        assert_eq!(
            observed,
            vec![("UPDATE".to_owned(), 1), ("DELETE".to_owned(), 2)]
        );
        let remaining = client
            .query(
                "SELECT id, label FROM compat_merge_by_source_target ORDER BY id",
                &[],
            )
            .await
            .expect("read the target after MERGE BY SOURCE");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].get::<_, i32>(0), 1);
        assert_eq!(remaining[0].get::<_, String>(1), "updated");

        drop(client);
        server.shutdown(connection).await;
    });
}
