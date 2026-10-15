use super::*;

#[test]
fn should_create_embedded_schema_elements_as_the_authorization_role() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("schema_authorization_elements").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE ROLE compat_schema_element_owner NOLOGIN",
                "CREATE SCHEMA compat_schema_element AUTHORIZATION \
                   compat_schema_element_owner \
                   CREATE TABLE embedded_rows (id INTEGER PRIMARY KEY)",
            ],
        )
        .await
        .expect("create a role-owned schema with an embedded table declaration");

        // Act
        let owners = client
            .query_one(
                "SELECT namespace.nspname, \
                        pg_catalog.pg_get_userbyid(namespace.nspowner), \
                        relation.relname, \
                        pg_catalog.pg_get_userbyid(relation.relowner) \
                 FROM pg_catalog.pg_namespace AS namespace \
                 JOIN pg_catalog.pg_class AS relation \
                   ON relation.relnamespace = namespace.oid \
                 WHERE namespace.nspname = 'compat_schema_element' \
                   AND relation.relname = 'embedded_rows' \
                   AND relation.relkind = 'r'",
                &[],
            )
            .await
            .expect("read the new schema and embedded table owners");
        client
            .batch_execute("SET ROLE compat_schema_element_owner")
            .await
            .expect("assume the schema authorization role");
        let inserted = client
            .query_one(
                "INSERT INTO compat_schema_element.embedded_rows VALUES (18) \
                 RETURNING current_user, id",
                &[],
            )
            .await
            .expect("insert into the embedded table as its owner");

        // Assert
        assert_eq!(owners.get::<_, String>(0), "compat_schema_element");
        assert_eq!(owners.get::<_, String>(1), "compat_schema_element_owner");
        assert_eq!(owners.get::<_, String>(2), "embedded_rows");
        assert_eq!(owners.get::<_, String>(3), "compat_schema_element_owner");
        assert_eq!(inserted.get::<_, String>(0), "compat_schema_element_owner");
        assert_eq!(inserted.get::<_, i32>(1), 18);

        drop(client);
        server.shutdown(connection).await;
    });
}
