#[test]
fn should_run_row_triggers_rollback_trigger_effects_with_the_statement() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("row_trigger_atomicity").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_trigger_source (id INT PRIMARY KEY)",
                "CREATE TABLE compat_trigger_log (id INT)",
                "CREATE FUNCTION compat_capture_insert() RETURNS TRIGGER \
                   LANGUAGE plpgsql AS $$ \
                   BEGIN \
                     INSERT INTO compat_trigger_log VALUES (NEW.id); \
                     RETURN NEW; \
                   END; $$",
                "CREATE TRIGGER compat_capture_insert_trigger \
                   AFTER INSERT ON compat_trigger_source FOR EACH ROW \
                   EXECUTE FUNCTION compat_capture_insert()",
            ],
        )
        .await
        .expect("create row trigger and function");

        // Act
        client
            .execute("INSERT INTO compat_trigger_source VALUES (9)", &[])
            .await
            .expect("insert row and run trigger");
        execute_statements(
            &client,
            &[
                "BEGIN",
                "INSERT INTO compat_trigger_source VALUES (10)",
                "ROLLBACK",
            ],
        )
        .await
        .expect("rollback row and its trigger effect together");
        let logged = client
            .query("SELECT id FROM compat_trigger_log ORDER BY id", &[])
            .await
            .expect("read trigger effects after rollback");

        // Assert
        assert_eq!(
            logged
                .iter()
                .map(|row| row.get::<_, i32>(0))
                .collect::<Vec<_>>(),
            vec![9],
            "rolling back a triggered insert also rolls back its log row"
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
