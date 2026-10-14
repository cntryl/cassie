use super::*;

#[test]
fn should_report_missing_trigger_function_before_trigger_return_type_resolution() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("trigger_or_replace_multievent").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_trigger_parse_rows (id INT)")
            .await
            .expect("create the table for trigger syntax resolution");

        // PostgreSQL checks the referenced function while processing the trigger declaration.
        // This statement is the only trigger DDL; no RETURNS TRIGGER routine is created.
        // Act
        let error = client
            .batch_execute(
                "CREATE OR REPLACE TRIGGER compat_missing_trigger_function \
                 BEFORE INSERT OR UPDATE OR DELETE ON compat_trigger_parse_rows \
                 FOR EACH ROW EXECUTE PROCEDURE public.compat_missing_trigger_function()",
            )
            .await
            .expect_err("the unresolved trigger function should be rejected");
        let row_count: i64 = client
            .query_one("SELECT count(*) FROM compat_trigger_parse_rows", &[])
            .await
            .expect("the connection should execute after the rejected declaration")
            .get(0);

        // Assert
        assert_eq!(row_count, 0);
        assert_eq!(db_error(&error).code().code(), "42883");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_ignore_dropping_an_absent_trigger_with_if_exists() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("trigger_drop_if_exists").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_trigger_drop_rows (id INT PRIMARY KEY)")
            .await
            .expect("create a table with no triggers");
        client
            .batch_execute("INSERT INTO compat_trigger_drop_rows VALUES (7)")
            .await
            .expect("populate the table before dropping an absent trigger");

        // Act
        client
            .batch_execute(
                "DROP TRIGGER IF EXISTS compat_absent_trigger \
                 ON compat_trigger_drop_rows",
            )
            .await
            .expect("dropping an absent trigger with IF EXISTS should succeed");
        let rows = client
            .query("SELECT id FROM compat_trigger_drop_rows ORDER BY id", &[])
            .await
            .expect("read the table after the no-op trigger drop");

        // Assert
        assert_eq!(
            rows.iter()
                .map(|row| row.get::<_, i32>(0))
                .collect::<Vec<_>>(),
            vec![7]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
