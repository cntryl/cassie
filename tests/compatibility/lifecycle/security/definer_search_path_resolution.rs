use super::*;

#[test]
fn should_resolve_security_definer_relation_before_callers_temporary_schema() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("security_definer_search_path").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE ROLE compat_search_path_reader NOLOGIN",
                "CREATE SCHEMA compat_search_path_trusted",
                "CREATE TABLE compat_search_path_trusted.secret (value TEXT NOT NULL)",
                "INSERT INTO compat_search_path_trusted.secret VALUES ('trusted-value')",
                "SET search_path TO compat_search_path_trusted, public",
                "CREATE FUNCTION compat_search_path_trusted.read_secret() RETURNS TEXT \
                   LANGUAGE SQL SECURITY DEFINER \
                   SET search_path = compat_search_path_trusted, pg_temp \
                   AS $$ SELECT current_user || ':' || value FROM secret $$",
                "REVOKE ALL ON FUNCTION compat_search_path_trusted.read_secret() FROM PUBLIC",
                "GRANT USAGE ON SCHEMA compat_search_path_trusted \
                   TO compat_search_path_reader",
                "GRANT EXECUTE ON FUNCTION compat_search_path_trusted.read_secret() \
                   TO compat_search_path_reader",
                "SET search_path TO public, pg_temp",
                "SET ROLE compat_search_path_reader",
                "CREATE TEMP TABLE secret (value TEXT NOT NULL)",
                "INSERT INTO secret VALUES ('caller-controlled-value')",
            ],
        )
        .await
        .expect("create a protected table, pinned definer routine and caller temp shadow");

        // Act
        let context = client
            .query_one(
                "SELECT current_user, \
                        compat_search_path_trusted.read_secret(), \
                        has_table_privilege(current_user, \
                          'compat_search_path_trusted.secret', 'SELECT'), \
                        current_setting('search_path')",
                &[],
            )
            .await
            .expect("call the routine through the reader role");
        let direct_read = client
            .query("SELECT value FROM compat_search_path_trusted.secret", &[])
            .await
            .expect_err("the caller must not read the protected relation directly");
        client
            .batch_execute("RESET ROLE")
            .await
            .expect("restore the administrative role");

        // Assert
        assert_eq!(context.get::<_, String>(0), "compat_search_path_reader");
        assert_eq!(context.get::<_, String>(1), "root:trusted-value");
        assert!(!context.get::<_, bool>(2));
        assert_eq!(context.get::<_, String>(3), "public, pg_temp");
        assert_eq!(db_error(&direct_read).code().code(), "42501");

        drop(client);
        server.shutdown(connection).await;
    });
}
