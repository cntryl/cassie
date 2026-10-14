use super::*;

#[test]
fn should_validate_named_not_null_added_not_valid_over_existing_nulls() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("named_not_null_not_valid_validation").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_named_nn_not_valid (id INT)",
                "INSERT INTO compat_named_nn_not_valid VALUES (NULL)",
                "ALTER TABLE compat_named_nn_not_valid \
                   ADD CONSTRAINT compat_named_nn_not_valid_key \
                   NOT NULL id NOT VALID",
            ],
        )
        .await
        .expect("add a named NOT NULL constraint without scanning the existing NULL");
        let initial = client
            .query_one(
                "SELECT conname, contype::TEXT, convalidated, conenforced, \
                        conkey::TEXT, pg_catalog.pg_get_constraintdef(oid) \
                 FROM pg_catalog.pg_constraint \
                 WHERE conname = 'compat_named_nn_not_valid_key'",
                &[],
            )
            .await
            .expect("inspect the unvalidated named NOT NULL constraint");

        // Act
        let rejected_new_null = client
            .execute("INSERT INTO compat_named_nn_not_valid VALUES (NULL)", &[])
            .await
            .expect_err("NOT VALID must still reject new NULL values");
        let rejected_validation = client
            .batch_execute(
                "ALTER TABLE compat_named_nn_not_valid \
                 VALIDATE CONSTRAINT compat_named_nn_not_valid_key",
            )
            .await
            .expect_err("validation must reject the existing NULL value");
        let after_failed_validation = client
            .query_one(
                "SELECT convalidated FROM pg_catalog.pg_constraint \
                 WHERE conname = 'compat_named_nn_not_valid_key'",
                &[],
            )
            .await
            .expect("inspect the constraint after failed validation")
            .get::<_, bool>(0);
        client
            .execute(
                "UPDATE compat_named_nn_not_valid SET id = 1 WHERE id IS NULL",
                &[],
            )
            .await
            .expect("repair the legacy NULL value");
        client
            .batch_execute(
                "ALTER TABLE compat_named_nn_not_valid \
                 VALIDATE CONSTRAINT compat_named_nn_not_valid_key",
            )
            .await
            .expect("validate the named NOT NULL after repairing existing data");
        let validated = client
            .query_one(
                "SELECT conname, contype::TEXT, convalidated, conenforced, \
                        conkey::TEXT, pg_catalog.pg_get_constraintdef(oid) \
                 FROM pg_catalog.pg_constraint \
                 WHERE conname = 'compat_named_nn_not_valid_key'",
                &[],
            )
            .await
            .expect("inspect the validated named NOT NULL constraint");
        let rows = client
            .query("SELECT id FROM compat_named_nn_not_valid ORDER BY id", &[])
            .await
            .expect("read the repaired relation row");

        // Assert
        assert_eq!(initial.get::<_, String>(0), "compat_named_nn_not_valid_key");
        assert_eq!(initial.get::<_, String>(1), "n");
        assert!(!initial.get::<_, bool>(2));
        assert!(initial.get::<_, bool>(3));
        assert_eq!(initial.get::<_, String>(4), "{1}");
        assert_eq!(initial.get::<_, String>(5), "NOT NULL id NOT VALID");
        assert_eq!(db_error(&rejected_new_null).code().code(), "23502");
        assert_eq!(db_error(&rejected_validation).code().code(), "23502");
        assert!(!after_failed_validation);
        assert_eq!(
            validated.get::<_, String>(0),
            "compat_named_nn_not_valid_key"
        );
        assert_eq!(validated.get::<_, String>(1), "n");
        assert!(validated.get::<_, bool>(2));
        assert!(validated.get::<_, bool>(3));
        assert_eq!(validated.get::<_, String>(4), "{1}");
        assert_eq!(validated.get::<_, String>(5), "NOT NULL id");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<_, i32>(0), 1);

        drop(client);
        server.shutdown(connection).await;
    });
}
