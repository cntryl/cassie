use super::*;

#[test]
fn should_recreate_relation_after_drop() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("recreate_dropped_relation").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_recreated_rows \
                 (id INT PRIMARY KEY, email TEXT, active BOOLEAN)",
                "CREATE UNIQUE INDEX compat_recreated_email_key \
                 ON compat_recreated_rows (email) WHERE active",
                "INSERT INTO compat_recreated_rows VALUES (1, 'old', TRUE)",
            ],
        )
        .await
        .expect("create and seed the relation before drop");
        client
            .batch_execute("DROP TABLE compat_recreated_rows")
            .await
            .expect("drop the original relation and its index");
        // Act
        client
            .batch_execute(
                "CREATE TABLE compat_recreated_rows \
                 (id INT PRIMARY KEY, email TEXT, active BOOLEAN)",
            )
            .await
            .expect("recreate the relation using the same name");
        client
            .batch_execute(
                "CREATE UNIQUE INDEX compat_recreated_email_key \
                 ON compat_recreated_rows (email) WHERE active",
            )
            .await
            .expect("recreate the index using the same name");
        client
            .batch_execute("INSERT INTO compat_recreated_rows VALUES (2, 'new', TRUE)")
            .await
            .expect("write to the recreated relation");
        let rows = client
            .query(
                "SELECT id, email, active FROM compat_recreated_rows ORDER BY id",
                &[],
            )
            .await
            .expect("read the recreated relation")
            .iter()
            .map(|row| {
                (
                    row.get::<_, i32>(0),
                    row.get::<_, String>(1),
                    row.get::<_, bool>(2),
                )
            })
            .collect::<Vec<_>>();

        // Assert
        assert_eq!(rows, vec![(2, "new".to_owned(), true)]);
        drop(client);
        server.shutdown(connection).await;
    });
}
