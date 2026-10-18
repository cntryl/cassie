#[test]
fn should_enforce_row_level_security_for_a_set_role_reader() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("row_level_security").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE ROLE compat_rls_reader NOLOGIN",
                "CREATE TABLE compat_rls_rows (id INT PRIMARY KEY, owner_name TEXT NOT NULL)",
                "INSERT INTO compat_rls_rows VALUES (1, 'compat_rls_reader'), (2, 'other')",
                "ALTER TABLE compat_rls_rows ENABLE ROW LEVEL SECURITY",
                "CREATE POLICY compat_rls_owner_policy ON compat_rls_rows \
                   FOR SELECT TO compat_rls_reader USING (owner_name = current_user)",
                "GRANT SELECT ON compat_rls_rows TO compat_rls_reader",
                "SET ROLE compat_rls_reader",
            ],
        )
        .await
        .expect("create and assume a row-scoped reader role");

        // Act
        let rows = client
            .query("SELECT id FROM compat_rls_rows ORDER BY id", &[])
            .await
            .expect("query through row security");
        client
            .batch_execute("RESET ROLE")
            .await
            .expect("restore session role");

        // Assert
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<_, i32>(0), 1);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_enforce_table_privileges_for_an_assumed_role() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("assumed_role_table_privileges").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE ROLE compat_assumed_reader NOLOGIN")
            .await
            .expect("create a role with limited table access");
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_assumed_privilege_rows (id INT PRIMARY KEY)",
                "INSERT INTO compat_assumed_privilege_rows VALUES (1)",
                "GRANT SELECT ON TABLE compat_assumed_privilege_rows TO compat_assumed_reader",
                "SET ROLE compat_assumed_reader",
            ],
        )
        .await
        .expect("grant read access and assume the grantee role");

        // Act
        let current_role: String = client
            .query_one("SELECT current_user", &[])
            .await
            .expect("read the active SQL role")
            .try_get(0)
            .expect("current role name");
        let visible: i32 = client
            .query_one("SELECT id FROM compat_assumed_privilege_rows", &[])
            .await
            .expect("read a row granted to the active role")
            .try_get(0)
            .expect("visible row id");
        let denied_write = client
            .execute("INSERT INTO compat_assumed_privilege_rows VALUES (2)", &[])
            .await
            .expect_err("a SELECT-only role cannot insert rows");
        client
            .batch_execute("RESET ROLE")
            .await
            .expect("restore the session's original role");

        // Assert
        assert_eq!(current_role, "compat_assumed_reader");
        assert_eq!(visible, 1);
        assert_eq!(db_error(&denied_write).code().code(), "42501");

        client
            .batch_execute("DROP TABLE compat_assumed_privilege_rows")
            .await
            .expect("drop the table privilege fixture");
        client
            .batch_execute("DROP ROLE compat_assumed_reader")
            .await
            .expect("drop the temporary privilege role");
        drop(client);
        server.shutdown(connection).await;
    });
}
