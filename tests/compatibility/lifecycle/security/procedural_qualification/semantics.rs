#[test]
fn should_apply_security_definer_privileges_without_granting_table_access() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("security_definer_table_privilege").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE ROLE compat_definer_reader NOLOGIN",
                "CREATE TABLE compat_definer_secret (value TEXT)",
                "INSERT INTO compat_definer_secret VALUES ('sealed')",
                "CREATE FUNCTION compat_definer_read_secret() RETURNS TEXT \
                   LANGUAGE SQL SECURITY DEFINER AS $$ \
                   SELECT value FROM public.compat_definer_secret $$",
                "GRANT EXECUTE ON FUNCTION compat_definer_read_secret() \
                   TO compat_definer_reader",
                "SET ROLE compat_definer_reader",
            ],
        )
        .await
        .expect("create the protected table and a callable SECURITY DEFINER routine");

        // Act
        let result = client
            .query_one(
                "SELECT compat_definer_read_secret(), \
                        has_table_privilege(current_user, \
                          'public.compat_definer_secret', 'SELECT')",
                &[],
            )
            .await
            .expect("read the protected value through the definer routine");
        let direct_read_error = client
            .query("SELECT value FROM compat_definer_secret", &[])
            .await
            .expect_err("the caller must not read the protected table directly");
        client
            .batch_execute("RESET ROLE")
            .await
            .expect("restore the administrative role");

        // Assert
        assert_eq!(result.get::<_, String>(0), "sealed");
        assert!(!result.get::<_, bool>(1));
        assert_eq!(db_error(&direct_read_error).code().code(), "42501");

        drop(client);
        server.shutdown(connection).await;
    });
}
