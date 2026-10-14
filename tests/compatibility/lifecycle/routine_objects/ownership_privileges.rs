use super::*;

#[test]
fn should_report_ungranted_object_privileges_as_false() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("ungranted_object_privileges").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE ROLE compat_ungranted_reader NOLOGIN",
                "CREATE SCHEMA compat_ungranted_schema",
                "CREATE TABLE compat_ungranted_schema.rows (id INT)",
            ],
        )
        .await
        .expect("create role and objects without granting privileges");

        // Act
        let privileges = client
            .query_one(
                "SELECT has_database_privilege('compat_ungranted_reader', 'postgres', 'CREATE'), \
                        has_schema_privilege('compat_ungranted_reader', 'compat_ungranted_schema', 'USAGE'), \
                        has_table_privilege('compat_ungranted_reader', 'compat_ungranted_schema.rows', 'SELECT')",
                &[],
            )
            .await
            .expect("check privileges for the ungranted role");

        // Assert
        assert_eq!(
            (
                privileges.get::<_, bool>(0),
                privileges.get::<_, bool>(1),
                privileges.get::<_, bool>(2),
            ),
            (false, false, false)
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_report_unrelated_roles_are_not_members() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("unrelated_role_membership").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE ROLE compat_unrelated_member NOLOGIN",
                "CREATE ROLE compat_unrelated_target NOLOGIN",
            ],
        )
        .await
        .expect("create independent roles without a membership grant");

        // Act
        let membership = client
            .query_one(
                "SELECT pg_has_role('compat_unrelated_member', 'compat_unrelated_target', 'MEMBER')",
                &[],
            )
            .await
            .expect("check membership between unrelated roles");

        // Assert
        assert!(!membership.get::<_, bool>(0));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_report_transferred_relation_owner_in_catalogs() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("relation_owner_transfer").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE ROLE compat_relation_owner NOLOGIN",
                "CREATE TABLE compat_owner_transfer_rows (id INT)",
            ],
        )
        .await
        .expect("create a target role and relation");

        // Act
        client
            .batch_execute("ALTER TABLE compat_owner_transfer_rows OWNER TO compat_relation_owner")
            .await
            .expect("transfer the table to the target owner");
        let owner: String = client
            .query_one(
                "SELECT pg_get_userbyid(relowner) FROM pg_class \
                 WHERE relname = 'compat_owner_transfer_rows'",
                &[],
            )
            .await
            .expect("read the relation owner from pg_class")
            .try_get(0)
            .expect("relation owner name");

        // Assert
        assert_eq!(owner, "compat_relation_owner");

        drop(client);
        server.shutdown(connection).await;
    });
}
