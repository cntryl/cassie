#[test]
fn should_apply_invoker_privileges_to_dynamic_sql_in_a_do_block() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("do_dynamic_sql_invoker_privileges").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE ROLE compat_do_exec_reader NOLOGIN",
                "CREATE TABLE compat_do_exec_rows (id INT PRIMARY KEY)",
                "GRANT SELECT ON TABLE compat_do_exec_rows TO compat_do_exec_reader",
                "SET ROLE compat_do_exec_reader",
            ],
        )
        .await
        .expect("create a read-only role and assume it");

        // Act
        let error = client
            .batch_execute(
                r"DO $block$
                   BEGIN
                     EXECUTE 'INSERT INTO compat_do_exec_rows VALUES (1)';
                   END;
                   $block$",
            )
            .await
            .expect_err("dynamic SQL in a DO block must use the invoker's table privileges");
        client
            .batch_execute("RESET ROLE")
            .await
            .expect("restore the administrative role after the denied statement");
        let rows = client
            .query("SELECT id FROM compat_do_exec_rows", &[])
            .await
            .expect("read the table after the rejected dynamic insert");

        // Assert
        assert_eq!(db_error(&error).code().code(), "42501");
        assert!(rows.is_empty(), "the denied insert must leave no row");

        drop(client);
        server.shutdown(connection).await;
    });
}
