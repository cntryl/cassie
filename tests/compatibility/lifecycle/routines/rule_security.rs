use super::*;

#[test]
fn should_enforce_select_privileges_given_a_rewrite_rule_when_a_restricted_role_queries_the_relation(
) {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("rewrite_select_privileges").await;
        let (client, connection) = server.connect().await;
        for statement in [
            "CREATE ROLE compat_rule_select_reader",
            "CREATE TABLE compat_rule_select_base (id INT, value TEXT)",
            "INSERT INTO compat_rule_select_base VALUES (1, 'visible through the view')",
            "CREATE VIEW compat_rule_select_view AS \
             SELECT id, value FROM compat_rule_select_base",
            "GRANT SELECT ON compat_rule_select_view TO compat_rule_select_reader",
        ] {
            client
                .batch_execute(statement)
                .await
                .expect("create a view rewrite and grant only view SELECT");
        }
        client
            .batch_execute("SET ROLE compat_rule_select_reader")
            .await
            .expect("switch to a role without access to the base table");

        // Act
        let rows = client
            .query("SELECT id, value FROM compat_rule_select_view", &[])
            .await
            .expect("read through the SELECT rewrite using view privileges");
        client
            .batch_execute("RESET ROLE")
            .await
            .expect("restore the session role to inspect base-table permissions");
        let base_select = client
            .query_one(
                "SELECT has_table_privilege(\
                   'compat_rule_select_reader', 'compat_rule_select_base', 'SELECT')",
                &[],
            )
            .await
            .expect("inspect the restricted role's base-table access")
            .get::<_, bool>(0);

        // Assert
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, String>(1), "visible through the view");
        assert!(!base_select);
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_enforce_update_privileges_given_a_rewrite_rule_when_a_restricted_role_updates_the_relation(
) {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("rewrite_update_privileges").await;
        let (client, connection) = server.connect().await;
        for statement in [
            "CREATE ROLE compat_rule_update_writer",
            "CREATE TABLE compat_rule_update_base (id INT PRIMARY KEY, value TEXT)",
            "INSERT INTO compat_rule_update_base VALUES (1, 'before')",
            "CREATE TABLE compat_rule_update_surface (id INT, value TEXT)",
            "INSERT INTO compat_rule_update_surface VALUES (1, 'before')",
            "CREATE RULE compat_rule_update AS ON UPDATE TO compat_rule_update_surface \
             DO INSTEAD UPDATE compat_rule_update_base \
             SET value = NEW.value WHERE id = OLD.id",
            "GRANT SELECT, UPDATE ON compat_rule_update_surface TO compat_rule_update_writer",
        ] {
            client
                .batch_execute(statement)
                .await
                .expect("create the UPDATE rewrite and grant only surface privileges");
        }
        client
            .batch_execute("SET ROLE compat_rule_update_writer")
            .await
            .expect("switch to a role without base-table UPDATE privileges");

        // Act
        client
            .execute(
                "UPDATE compat_rule_update_surface SET value = 'after' WHERE id = 1",
                &[],
            )
            .await
            .expect("apply the UPDATE rewrite using relation privileges");
        client
            .batch_execute("RESET ROLE")
            .await
            .expect("restore the session role to inspect the rewritten update");
        let updated = client
            .query_one(
                "SELECT value, has_table_privilege(\
                   'compat_rule_update_writer', 'compat_rule_update_base', 'UPDATE') \
                 FROM compat_rule_update_base WHERE id = 1",
                &[],
            )
            .await
            .expect("read base data and the restricted role's base privilege");

        // Assert
        assert_eq!(updated.get::<_, String>(0), "after");
        assert!(!updated.get::<_, bool>(1));
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_enforce_delete_privileges_given_a_rewrite_rule_when_a_restricted_role_deletes_from_the_relation(
) {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("rewrite_delete_privileges").await;
        let (client, connection) = server.connect().await;
        for statement in [
            "CREATE ROLE compat_rule_delete_writer",
            "CREATE TABLE compat_rule_delete_base (id INT PRIMARY KEY, value TEXT)",
            "INSERT INTO compat_rule_delete_base VALUES (1, 'remove')",
            "CREATE TABLE compat_rule_delete_surface (id INT, value TEXT)",
            "INSERT INTO compat_rule_delete_surface VALUES (1, 'remove')",
            "CREATE RULE compat_rule_delete AS ON DELETE TO compat_rule_delete_surface \
             DO INSTEAD DELETE FROM compat_rule_delete_base WHERE id = OLD.id",
            "GRANT SELECT, DELETE ON compat_rule_delete_surface TO compat_rule_delete_writer",
        ] {
            client
                .batch_execute(statement)
                .await
                .expect("create the DELETE rewrite and grant only surface privileges");
        }
        client
            .batch_execute("SET ROLE compat_rule_delete_writer")
            .await
            .expect("switch to a role without base-table DELETE privileges");

        // Act
        client
            .execute("DELETE FROM compat_rule_delete_surface WHERE id = 1", &[])
            .await
            .expect("apply the DELETE rewrite using relation privileges");
        client
            .batch_execute("RESET ROLE")
            .await
            .expect("restore the session role to inspect the rewritten delete");
        let remaining = client
            .query_one(
                "SELECT count(*)::BIGINT, has_table_privilege(\
                   'compat_rule_delete_writer', 'compat_rule_delete_base', 'DELETE') \
                 FROM compat_rule_delete_base",
                &[],
            )
            .await
            .expect("read base data and the restricted role's base privilege");

        // Assert
        assert_eq!(remaining.get::<_, i64>(0), 0);
        assert!(!remaining.get::<_, bool>(1));
        drop(client);
        server.shutdown(connection).await;
    });
}
