use super::*;

#[test]
fn should_enforce_column_select_grants_independently_of_table_select() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("column_select_privilege_scope").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE ROLE compat_column_reader NOLOGIN",
                "CREATE SCHEMA compat_column_scope",
                "CREATE TABLE compat_column_scope.rows \
                   (id INTEGER PRIMARY KEY, public_label TEXT NOT NULL, \
                    private_label TEXT NOT NULL)",
                "INSERT INTO compat_column_scope.rows \
                   VALUES (1, 'visible', 'secret')",
                "GRANT USAGE ON SCHEMA compat_column_scope TO compat_column_reader",
                "GRANT SELECT (id, public_label) \
                   ON TABLE compat_column_scope.rows TO compat_column_reader",
            ],
        )
        .await
        .expect("create a role with SELECT on only selected table columns");

        // Act
        let privileges = client
            .query_one(
                "SELECT has_column_privilege( \
                          'compat_column_reader', \
                          'compat_column_scope.rows', 'id', 'SELECT'), \
                        has_column_privilege( \
                          'compat_column_reader', \
                          'compat_column_scope.rows', 'public_label', 'SELECT'), \
                        has_column_privilege( \
                          'compat_column_reader', \
                          'compat_column_scope.rows', 'private_label', 'SELECT'), \
                        has_table_privilege( \
                          'compat_column_reader', \
                          'compat_column_scope.rows', 'SELECT')",
                &[],
            )
            .await
            .expect("inspect column-specific and table-level SELECT privileges");
        client
            .batch_execute("SET ROLE compat_column_reader")
            .await
            .expect("assume the role with only column-specific SELECT grants");
        let allowed_rows = client
            .query(
                "SELECT id, public_label FROM compat_column_scope.rows WHERE id = 1",
                &[],
            )
            .await
            .expect("read the granted columns");
        let denied_column = client
            .query(
                "SELECT private_label FROM compat_column_scope.rows WHERE id = 1",
                &[],
            )
            .await
            .expect_err("reject reading a column without a SELECT grant");

        // Assert
        assert_eq!(
            (
                privileges.get::<_, bool>(0),
                privileges.get::<_, bool>(1),
                privileges.get::<_, bool>(2),
                privileges.get::<_, bool>(3),
            ),
            (true, true, false, false)
        );
        assert_eq!(allowed_rows.len(), 1);
        assert_eq!(allowed_rows[0].columns()[0].type_().oid(), 23);
        assert_eq!(allowed_rows[0].columns()[1].type_().oid(), 25);
        assert_eq!(allowed_rows[0].get::<_, i32>(0), 1);
        assert_eq!(allowed_rows[0].get::<_, String>(1), "visible");
        assert_eq!(db_error(&denied_column).code().code(), "42501");

        drop(client);
        server.shutdown(connection).await;
    });
}
