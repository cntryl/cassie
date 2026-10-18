use super::*;

#[test]
fn should_transfer_schema_owner_without_reassigning_existing_objects() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("schema_owner_transfer").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE ROLE compat_838_schema_before NOLOGIN",
                "CREATE ROLE compat_838_schema_after NOLOGIN",
                "CREATE SCHEMA compat_838_owner_transfer \
                   AUTHORIZATION compat_838_schema_before",
                "SET ROLE compat_838_schema_before",
                "CREATE TABLE compat_838_owner_transfer.owned_rows (id INTEGER)",
                "RESET ROLE",
            ],
        )
        .await
        .expect("create a role-owned schema and a table owned by that role");
        let before = client
            .query_one(
                "SELECT namespace.oid, relation.oid \
                 FROM pg_catalog.pg_namespace AS namespace \
                 JOIN pg_catalog.pg_class AS relation \
                   ON relation.relnamespace = namespace.oid \
                 WHERE namespace.nspname = 'compat_838_owner_transfer' \
                   AND relation.relname = 'owned_rows' \
                   AND relation.relkind = 'r'",
                &[],
            )
            .await
            .expect("read the schema and table identities before transfer");

        // Act
        client
            .batch_execute(
                "ALTER SCHEMA compat_838_owner_transfer \
                 OWNER TO compat_838_schema_after",
            )
            .await
            .expect("transfer ownership of the existing schema");
        let after = client
            .query_one(
                "SELECT namespace.oid, relation.oid, \
                        pg_catalog.pg_get_userbyid(namespace.nspowner), \
                        pg_catalog.pg_get_userbyid(relation.relowner) \
                 FROM pg_catalog.pg_namespace AS namespace \
                 JOIN pg_catalog.pg_class AS relation \
                   ON relation.relnamespace = namespace.oid \
                 WHERE namespace.nspname = 'compat_838_owner_transfer' \
                   AND relation.relname = 'owned_rows' \
                   AND relation.relkind = 'r'",
                &[],
            )
            .await
            .expect("read the schema and table identities after transfer");
        client
            .batch_execute(
                "DROP SCHEMA compat_838_owner_transfer CASCADE; \
                 DROP ROLE compat_838_schema_before; \
                 DROP ROLE compat_838_schema_after",
            )
            .await
            .expect("remove the schema and both fixture roles");

        // Assert
        assert_eq!(before.get::<_, u32>(0), after.get::<_, u32>(0));
        assert_eq!(before.get::<_, u32>(1), after.get::<_, u32>(1));
        assert_eq!(after.get::<_, String>(2), "compat_838_schema_after");
        assert_eq!(after.get::<_, String>(3), "compat_838_schema_before");

        drop(client);
        server.shutdown(connection).await;
    });
}
