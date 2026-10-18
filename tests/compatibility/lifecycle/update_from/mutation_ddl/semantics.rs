#[test]
fn should_update_rows_from_a_joined_source() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("update_from_join").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_mutation_target (id INT PRIMARY KEY, label TEXT)",
                "CREATE TABLE compat_mutation_source (id INT PRIMARY KEY, label TEXT)",
                "INSERT INTO compat_mutation_target VALUES (1, 'old-a'), (2, 'old-b'), (3, 'keep')",
                "INSERT INTO compat_mutation_source VALUES (1, 'new-a'), (2, 'new-b')",
            ],
        )
        .await
        .expect("create joined-mutation fixtures");

        // Act
        let updated = client
            .query(
                "UPDATE compat_mutation_target AS target \
                 SET label = source.label \
                 FROM compat_mutation_source AS source \
                 WHERE target.id = source.id \
                 RETURNING target.id, target.label",
                &[],
            )
            .await
            .expect("update from a joined source");
        // Assert
        let mut updated_rows = updated
            .iter()
            .map(|row| (row.get::<_, i32>(0), row.get::<_, String>(1)))
            .collect::<Vec<_>>();
        updated_rows.sort();
        assert_eq!(
            updated_rows,
            vec![(1, "new-a".to_string()), (2, "new-b".to_string())]
        );
        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_assign_multiple_columns_from_a_joined_mutation_source() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("update_from_tuple_assignment").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_tuple_target (id INT PRIMARY KEY, amount INT, label TEXT)",
                "CREATE TABLE compat_tuple_source (id INT, amount INT, label TEXT)",
                "INSERT INTO compat_tuple_target VALUES (1, 10, 'old-a'), (2, 20, 'old-b'), (3, 30, 'keep')",
                "INSERT INTO compat_tuple_source VALUES (1, 11, 'new-a'), (2, 22, 'new-b')",
            ],
        )
        .await
        .expect("create joined tuple-assignment fixtures");

        // Act
        let updated = client
            .query(
                "UPDATE compat_tuple_target AS target \
                 SET (amount, label) = (source.amount, source.label) \
                 FROM compat_tuple_source AS source \
                 WHERE target.id = source.id \
                 RETURNING target.id, target.amount, target.label",
                &[],
            )
            .await
            .expect("assign a tuple from the joined source");
        assert_eq!(
            updated[0]
                .columns()
                .iter()
                .map(|column| column.type_().oid())
                .collect::<Vec<_>>(),
            vec![23, 23, 25]
        );
        let mut updated_rows = updated
            .iter()
            .map(|row| {
                (
                    row.get::<_, i32>(0),
                    row.get::<_, i32>(1),
                    row.get::<_, String>(2),
                )
            })
            .collect::<Vec<_>>();
        updated_rows.sort();
        let unchanged = client
            .query_one(
                "SELECT amount, label FROM compat_tuple_target WHERE id = 3",
                &[],
            )
            .await
            .expect("read the nonmatching target row");

        // Assert
        assert_eq!(
            updated_rows,
            vec![
                (1, 11, "new-a".to_string()),
                (2, 22, "new-b".to_string()),
            ]
        );
        assert_eq!(unchanged.get::<_, i32>(0), 30);
        assert_eq!(unchanged.get::<_, String>(1), "keep");

        drop(client);
        server.shutdown(connection).await;
    });
}
