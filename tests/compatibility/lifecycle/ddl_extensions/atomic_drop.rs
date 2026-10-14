use super::*;

#[test]
fn should_keep_all_drop_targets_when_a_later_target_is_missing() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("multi_drop_atomicity").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_multi_drop_first (id INT PRIMARY KEY)",
                "CREATE TABLE compat_multi_drop_second (id INT PRIMARY KEY)",
                "INSERT INTO compat_multi_drop_first VALUES (1)",
                "INSERT INTO compat_multi_drop_second VALUES (2)",
            ],
        )
        .await
        .expect("create populated DROP TABLE targets");

        // Act
        let drop_result = client
            .batch_execute("DROP TABLE compat_multi_drop_first, compat_multi_drop_missing")
            .await;
        let sqlstate = drop_result
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code().to_owned());
        let first_rows = client
            .query("SELECT id FROM compat_multi_drop_first ORDER BY id", &[])
            .await;
        let second_rows = client
            .query("SELECT id FROM compat_multi_drop_second ORDER BY id", &[])
            .await;

        drop(client);
        server.shutdown(connection).await;

        // Assert
        assert_eq!(sqlstate.as_deref(), Some("42P01"));
        let first_rows = first_rows.expect("read the first table after the rejected multi-drop");
        let second_rows = second_rows.expect("read the second table after the rejected multi-drop");
        assert_eq!(
            first_rows
                .iter()
                .map(|row| row.get::<_, i32>(0))
                .collect::<Vec<_>>(),
            vec![1],
            "DROP TABLE must not remove an earlier target before a later target fails"
        );
        assert_eq!(
            second_rows
                .iter()
                .map(|row| row.get::<_, i32>(0))
                .collect::<Vec<_>>(),
            vec![2]
        );
    });
}
