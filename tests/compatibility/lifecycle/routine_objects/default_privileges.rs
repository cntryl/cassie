use super::*;

#[test]
fn should_apply_default_table_privileges_to_future_relations() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("default_table_privileges").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE ROLE compat_default_priv_reader NOLOGIN")
            .await
            .expect("create a role to receive default table privileges");
        client
            .batch_execute(
                "ALTER DEFAULT PRIVILEGES IN SCHEMA public \
                 GRANT SELECT ON TABLES TO compat_default_priv_reader",
            )
            .await
            .expect("grant SELECT on future tables in the schema");
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_default_priv_rows (id INT PRIMARY KEY)",
                "INSERT INTO compat_default_priv_rows VALUES (7)",
                "SET ROLE compat_default_priv_reader",
            ],
        )
        .await
        .expect("create a relation after configuring defaults and assume the grantee role");

        // Act
        let visible_rows = client
            .query("SELECT id FROM compat_default_priv_rows", &[])
            .await
            .expect("the default grant should let the role read the new relation");
        client
            .batch_execute("RESET ROLE")
            .await
            .expect("restore the administrative role");

        // Assert
        assert_eq!(visible_rows.len(), 1);
        assert_eq!(visible_rows[0].get::<_, i32>(0), 7);

        client
            .batch_execute("DROP TABLE compat_default_priv_rows")
            .await
            .expect("drop the privilege fixture relation");
        client
            .batch_execute(
                "ALTER DEFAULT PRIVILEGES IN SCHEMA public \
                 REVOKE SELECT ON TABLES FROM compat_default_priv_reader",
            )
            .await
            .expect("remove the fixture default grant");
        client
            .batch_execute("DROP ROLE compat_default_priv_reader")
            .await
            .expect("drop the privilege fixture role");
        drop(client);
        server.shutdown(connection).await;
    });
}
