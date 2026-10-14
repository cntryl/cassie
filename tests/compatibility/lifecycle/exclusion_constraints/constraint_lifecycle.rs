use super::*;

#[test]
fn should_enforce_composed_named_check_predicates() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("composed_named_check_constraint").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_composed_check (state TEXT, score INT, label TEXT, active BOOL, \
                   CONSTRAINT compat_composed_check_name CHECK (CASE \
                     WHEN state IN ('active', 'queued') THEN \
                       score BETWEEN 0 AND 10 AND score IS NOT NULL \
                       AND label IS NOT NULL AND length(label) > 0 \
                       AND (active IS TRUE OR active IS FALSE) \
                     WHEN state = 'inactive' THEN \
                       score IS NULL AND label IS NULL AND active IS NULL \
                     ELSE FALSE END))",
                "INSERT INTO compat_composed_check VALUES \
                   ('active', 5, 'ready', TRUE), ('queued', 0, 'waiting', FALSE), \
                   ('inactive', NULL, NULL, NULL)",
            ],
        )
        .await
        .expect("create and seed the composed named CHECK fixture");

        // Act
        let rejected = [
            "INSERT INTO compat_composed_check VALUES ('active', 11, 'too high', TRUE)",
            "INSERT INTO compat_composed_check VALUES ('active', 5, '', TRUE)",
            "INSERT INTO compat_composed_check VALUES ('active', NULL, 'missing score', TRUE)",
            "INSERT INTO compat_composed_check VALUES ('active', 5, NULL, TRUE)",
            "INSERT INTO compat_composed_check VALUES ('active', 5, 'missing flag', NULL)",
            "INSERT INTO compat_composed_check VALUES ('paused', 5, 'unknown state', TRUE)",
            "INSERT INTO compat_composed_check VALUES (NULL, 5, 'missing state', TRUE)",
        ];
        for insert in rejected {
            let error = client
                .execute(insert, &[])
                .await
                .expect_err("every invalid branch of the CHECK must reject its row");
            assert_eq!(db_error(&error).code().code(), "23514", "{insert}");
        }
        let remaining: i64 = client
            .query_one("SELECT count(*) FROM compat_composed_check", &[])
            .await
            .expect("count rows after rejected CHECK inputs")
            .try_get(0)
            .expect("row count");
        let catalog = client
            .query_one(
                "SELECT contype::TEXT, convalidated FROM pg_catalog.pg_constraint \
                 WHERE conname = 'compat_composed_check_name'",
                &[],
            )
            .await
            .expect("inspect the named CHECK catalog entry");

        // Assert
        assert_eq!(remaining, 3);
        assert_eq!(catalog.get::<_, String>(0), "c");
        assert!(catalog.get::<_, bool>(1));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_complete_named_check_validation_rename_drop_lifecycle() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("named_check_constraint_lifecycle").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_check_lifecycle (id INT, score INT)",
                "INSERT INTO compat_check_lifecycle VALUES (1, 3), (2, 8)",
                "ALTER TABLE compat_check_lifecycle ADD CONSTRAINT \
                   compat_check_lifecycle_name CHECK (score >= 0) NOT VALID",
            ],
        )
        .await
        .expect("add a named NOT VALID CHECK over existing valid rows");
        let initially_validated: bool = client
            .query_one(
                "SELECT convalidated FROM pg_catalog.pg_constraint \
                 WHERE conname = 'compat_check_lifecycle_name'",
                &[],
            )
            .await
            .expect("inspect the new constraint validation state")
            .try_get(0)
            .expect("validation state");

        // Act
        client
            .batch_execute(
                "ALTER TABLE compat_check_lifecycle \
                 VALIDATE CONSTRAINT compat_check_lifecycle_name",
            )
            .await
            .expect("validate a CHECK whose existing rows satisfy its predicate");
        let validated: bool = client
            .query_one(
                "SELECT convalidated FROM pg_catalog.pg_constraint \
                 WHERE conname = 'compat_check_lifecycle_name'",
                &[],
            )
            .await
            .expect("inspect successful validation state")
            .try_get(0)
            .expect("validation state");
        client
            .batch_execute(
                "BEGIN; ALTER TABLE compat_check_lifecycle \
                 RENAME CONSTRAINT compat_check_lifecycle_name \
                 TO compat_check_lifecycle_rolled_back; ROLLBACK",
            )
            .await
            .expect("roll back a constraint rename");
        let original_name_survived: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_constraint \
                 WHERE conname = 'compat_check_lifecycle_name' AND convalidated",
                &[],
            )
            .await
            .expect("inspect the constraint name after rollback")
            .try_get(0)
            .expect("constraint count");
        client
            .batch_execute(
                "ALTER TABLE compat_check_lifecycle \
                 RENAME CONSTRAINT compat_check_lifecycle_name \
                 TO compat_check_lifecycle_renamed",
            )
            .await
            .expect("rename the validated CHECK constraint");
        let renamed_validated: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_constraint \
                 WHERE conname = 'compat_check_lifecycle_renamed' AND convalidated",
                &[],
            )
            .await
            .expect("inspect the renamed constraint")
            .try_get(0)
            .expect("renamed constraint count");
        client
            .batch_execute(
                "ALTER TABLE compat_check_lifecycle \
                 DROP CONSTRAINT compat_check_lifecycle_renamed",
            )
            .await
            .expect("drop the named CHECK constraint");
        let remaining_constraints: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_constraint \
                 WHERE conname IN ('compat_check_lifecycle_name', \
                                   'compat_check_lifecycle_renamed')",
                &[],
            )
            .await
            .expect("inspect the catalog after dropping the CHECK")
            .try_get(0)
            .expect("remaining matching constraints");

        // Assert
        assert!(!initially_validated);
        assert!(validated);
        assert_eq!(original_name_survived, 1);
        assert_eq!(renamed_validated, 1);
        assert_eq!(remaining_constraints, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_alter_named_foreign_key_mode_with_savepoint_recovery() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("alter_foreign_key_deferrability").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_alter_fk_parent (id INT PRIMARY KEY)",
                "CREATE TABLE compat_alter_fk_child (parent_id INT)",
                "ALTER TABLE compat_alter_fk_child ADD CONSTRAINT compat_alter_fk_name \
                   FOREIGN KEY (parent_id) REFERENCES compat_alter_fk_parent (id)",
                "ALTER TABLE compat_alter_fk_child ALTER CONSTRAINT compat_alter_fk_name \
                   DEFERRABLE INITIALLY DEFERRED",
            ],
        )
        .await
        .expect("create and alter the named foreign key");
        let metadata = client
            .query_one(
                "SELECT condeferrable, condeferred FROM pg_catalog.pg_constraint \
                 WHERE conname = 'compat_alter_fk_name'",
                &[],
            )
            .await
            .expect("inspect the altered deferrability metadata");
        client
            .batch_execute(
                "BEGIN; SET CONSTRAINTS ALL DEFERRED; \
                 SET CONSTRAINTS compat_alter_fk_name DEFERRED; \
                 SAVEPOINT before_invalid_child",
            )
            .await
            .expect("defer all constraints and the named foreign key");

        // Act
        client
            .execute("INSERT INTO compat_alter_fk_child VALUES (9)", &[])
            .await
            .expect("a deferred foreign key permits a pending violation");
        let forced_immediate = client
            .batch_execute("SET CONSTRAINTS compat_alter_fk_name IMMEDIATE")
            .await
            .expect_err("forcing the named foreign key immediate must check pending rows");
        client
            .batch_execute("ROLLBACK TO SAVEPOINT before_invalid_child")
            .await
            .expect("recover the transaction after the deferred check fails");
        let remaining: i64 = client
            .query_one("SELECT count(*) FROM compat_alter_fk_child", &[])
            .await
            .expect("inspect child rows after savepoint rollback")
            .try_get(0)
            .expect("row count");
        client
            .batch_execute("SET CONSTRAINTS ALL IMMEDIATE; COMMIT")
            .await
            .expect("set all constraints immediate and commit the recovered transaction");

        // Assert
        assert!(metadata.get::<_, bool>(0));
        assert!(metadata.get::<_, bool>(1));
        assert_eq!(db_error(&forced_immediate).code().code(), "23503");
        assert_eq!(remaining, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_toggle_named_foreign_key_enforcement_after_repairing_existing_rows() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("foreign_key_enforcement_lifecycle").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_toggle_fk_parent (id INT PRIMARY KEY)",
                "CREATE TABLE compat_toggle_fk_child (parent_id INT, \
                   CONSTRAINT compat_toggle_fk_name FOREIGN KEY (parent_id) \
                   REFERENCES compat_toggle_fk_parent (id) NOT ENFORCED)",
                "INSERT INTO compat_toggle_fk_child VALUES (9)",
            ],
        )
        .await
        .expect("create a NOT ENFORCED foreign key and accept its existing violation");
        let initially_unenforced = client
            .query_one(
                "SELECT conenforced, convalidated FROM pg_catalog.pg_constraint \
                 WHERE conname = 'compat_toggle_fk_name'",
                &[],
            )
            .await
            .expect("inspect initial NOT ENFORCED foreign key state");

        // Act
        let failed_enable = client
            .batch_execute(
                "ALTER TABLE compat_toggle_fk_child \
                 ALTER CONSTRAINT compat_toggle_fk_name ENFORCED",
            )
            .await
            .expect_err("ENFORCED must validate existing foreign key rows");
        let state_after_failed_enable = client
            .query_one(
                "SELECT conenforced, convalidated FROM pg_catalog.pg_constraint \
                 WHERE conname = 'compat_toggle_fk_name'",
                &[],
            )
            .await
            .expect("inspect state after failed enforcement transition");
        client
            .execute(
                "DELETE FROM compat_toggle_fk_child WHERE parent_id = 9",
                &[],
            )
            .await
            .expect("remove the existing foreign key violation");
        client
            .batch_execute(
                "ALTER TABLE compat_toggle_fk_child \
                 ALTER CONSTRAINT compat_toggle_fk_name ENFORCED",
            )
            .await
            .expect("enforce the foreign key after repairing existing rows");
        let enforced_state = client
            .query_one(
                "SELECT conenforced, convalidated FROM pg_catalog.pg_constraint \
                 WHERE conname = 'compat_toggle_fk_name'",
                &[],
            )
            .await
            .expect("inspect successful enforcement transition");
        let rejected_new_violation = client
            .execute("INSERT INTO compat_toggle_fk_child VALUES (9)", &[])
            .await
            .expect_err("an enforced foreign key must reject a new violation");
        client
            .batch_execute(
                "ALTER TABLE compat_toggle_fk_child \
                 ALTER CONSTRAINT compat_toggle_fk_name NOT ENFORCED",
            )
            .await
            .expect("disable enforcement on the named foreign key");
        client
            .execute("INSERT INTO compat_toggle_fk_child VALUES (9)", &[])
            .await
            .expect("a NOT ENFORCED foreign key permits a new violation");
        let final_state = client
            .query_one(
                "SELECT conenforced, convalidated FROM pg_catalog.pg_constraint \
                 WHERE conname = 'compat_toggle_fk_name'",
                &[],
            )
            .await
            .expect("inspect the final NOT ENFORCED state");
        let remaining: i64 = client
            .query_one("SELECT count(*) FROM compat_toggle_fk_child", &[])
            .await
            .expect("count the row accepted after disabling enforcement")
            .try_get(0)
            .expect("row count");

        // Assert
        assert!(!initially_unenforced.get::<_, bool>(0));
        assert!(!initially_unenforced.get::<_, bool>(1));
        assert_eq!(db_error(&failed_enable).code().code(), "23503");
        assert!(!state_after_failed_enable.get::<_, bool>(0));
        assert!(!state_after_failed_enable.get::<_, bool>(1));
        assert!(enforced_state.get::<_, bool>(0));
        assert!(enforced_state.get::<_, bool>(1));
        assert_eq!(db_error(&rejected_new_violation).code().code(), "23503");
        assert!(!final_state.get::<_, bool>(0));
        assert!(!final_state.get::<_, bool>(1));
        assert_eq!(remaining, 1);

        drop(client);
        server.shutdown(connection).await;
    });
}
