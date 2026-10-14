use super::*;

#[test]
fn should_apply_using_expression_when_altering_column_type() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("alter_column_using").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_using_rows (id INT PRIMARY KEY, amount INT)",
                "INSERT INTO compat_using_rows VALUES (1, 22), (2, 33)",
            ],
        )
        .await
        .expect("create rows with an integer column to transform");

        // Act
        client
            .batch_execute(
                "ALTER TABLE compat_using_rows ALTER COLUMN amount TYPE TEXT \
                 USING ('amount=' || amount::TEXT)",
            )
            .await
            .expect("convert existing values through the USING expression");
        let rows = client
            .query("SELECT id, amount FROM compat_using_rows ORDER BY id", &[])
            .await
            .expect("read the converted rows");

        // Assert
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, String>(1), "amount=22");
        assert_eq!(rows[1].get::<_, i32>(0), 2);
        assert_eq!(rows[1].get::<_, String>(1), "amount=33");

        drop(client);
        server.shutdown(connection).await;
    });
}
