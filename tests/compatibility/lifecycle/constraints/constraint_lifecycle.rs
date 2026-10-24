use super::*;

fn assert_foreign_key_enforcement_lifecycle(
    initially_unenforced: &tokio_postgres::Row,
    failed_enable: &tokio_postgres::Error,
    state_after_failed_enable: &tokio_postgres::Row,
    enforced_state: &tokio_postgres::Row,
    rejected_new_violation: &tokio_postgres::Error,
    final_state: &tokio_postgres::Row,
    remaining: i64,
) {
    assert!(!initially_unenforced.get::<_, bool>(0));
    assert!(!initially_unenforced.get::<_, bool>(1));
    assert_eq!(db_error(failed_enable).code().code(), "23503");
    assert!(!state_after_failed_enable.get::<_, bool>(0));
    assert!(!state_after_failed_enable.get::<_, bool>(1));
    assert!(enforced_state.get::<_, bool>(0));
    assert!(enforced_state.get::<_, bool>(1));
    assert_eq!(db_error(rejected_new_violation).code().code(), "23503");
    assert!(!final_state.get::<_, bool>(0));
    assert!(!final_state.get::<_, bool>(1));
    assert_eq!(remaining, 1);
}

async fn run_foreign_key_enforcement_lifecycle(
    client: &tokio_postgres::Client,
) -> (
    tokio_postgres::Row,
    tokio_postgres::Error,
    tokio_postgres::Row,
    tokio_postgres::Row,
    tokio_postgres::Error,
    tokio_postgres::Row,
    i64,
) {
    let initially_unenforced = create_unenforced_foreign_key_fixture(client).await;
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
    let remaining = client
        .query_one("SELECT count(*) FROM compat_toggle_fk_child", &[])
        .await
        .expect("count the row accepted after disabling enforcement")
        .get(0);
    (
        initially_unenforced,
        failed_enable,
        state_after_failed_enable,
        enforced_state,
        rejected_new_violation,
        final_state,
        remaining,
    )
}

async fn create_unenforced_foreign_key_fixture(
    client: &tokio_postgres::Client,
) -> tokio_postgres::Row {
    execute_statements(
        client,
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
    client
        .query_one(
            "SELECT conenforced, convalidated FROM pg_catalog.pg_constraint \
             WHERE conname = 'compat_toggle_fk_name'",
            &[],
        )
        .await
        .expect("inspect initial NOT ENFORCED foreign key state")
}

#[path = "extended_constraint_forms.rs"]
mod extended_constraint_forms;
#[path = "named_not_null_validation.rs"]
mod named_not_null_validation;

#[test]
fn should_reject_composed_named_check_predicates_until_supported() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("composed_named_check_constraint").await;
        let (client, connection) = server.connect().await;
        let create_error = client
            .batch_execute(
                "CREATE TABLE compat_composed_check \
                   (state TEXT, score INT, label TEXT, active BOOL, \
                    CONSTRAINT compat_composed_check_name CHECK (CASE \
                      WHEN state IN ('active', 'queued') THEN \
                        score BETWEEN 0 AND 10 AND score IS NOT NULL \
                        AND label IS NOT NULL AND length(label) > 0 \
                        AND (active IS TRUE OR active IS FALSE) \
                      WHEN state = 'inactive' THEN \
                        score IS NULL AND label IS NULL AND active IS NULL \
                      ELSE FALSE END))",
            )
            .await
            .expect_err("general composed CHECK expressions remain unsupported");

        // Act
        let relation_exists: bool = client
            .query_one(
                "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
                 WHERE table_name = 'compat_composed_check')",
                &[],
            )
            .await
            .expect("check that rejected DDL did not publish a relation")
            .get(0);

        // Assert
        assert_eq!(db_error(&create_error).code().code(), "0A000");
        assert!(!relation_exists);

        drop(client);
        server.shutdown(connection).await;
    });
}

async fn create_named_check_fixture(client: &tokio_postgres::Client) {
    execute_statements(
        client,
        &[
            "CREATE TABLE compat_check_lifecycle (id INT, score INT)",
            "INSERT INTO compat_check_lifecycle VALUES (1, 3), (2, 8)",
            "ALTER TABLE compat_check_lifecycle ADD CONSTRAINT \
               compat_check_lifecycle_name CHECK (score >= 0)",
        ],
    )
    .await
    .expect("add a named CHECK before probing unsupported lifecycle forms");
}

#[test]
fn should_reject_check_validation_without_mutating_catalog() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("named_check_validation_unsupported").await;
        let (client, connection) = server.connect().await;
        create_named_check_fixture(&client).await;

        // Act
        let validation_error = client
            .batch_execute(
                "ALTER TABLE compat_check_lifecycle \
                 VALIDATE CONSTRAINT compat_check_lifecycle_name",
            )
            .await
            .expect_err("CHECK validation remains outside the selected constraint profile");
        let original_constraints: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_constraint \
                 WHERE conname = 'compat_check_lifecycle_name'",
                &[],
            )
            .await
            .expect("inspect the original constraint after rejected commands")
            .get(0);

        // Assert
        assert_eq!(db_error(&validation_error).code().code(), "0A000");
        assert_eq!(original_constraints, 1);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_check_constraint_rename_without_mutating_catalog() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("named_check_rename_unsupported").await;
        let (client, connection) = server.connect().await;
        create_named_check_fixture(&client).await;

        // Act
        let rename_error = client
            .batch_execute(
                "ALTER TABLE compat_check_lifecycle \
                 RENAME CONSTRAINT compat_check_lifecycle_name \
                 TO compat_check_lifecycle_renamed",
            )
            .await
            .expect_err("CHECK constraint rename remains unsupported");
        let original_constraints: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_constraint \
                 WHERE conname = 'compat_check_lifecycle_name'",
                &[],
            )
            .await
            .expect("inspect the original constraint after the rejected rename")
            .get(0);
        let renamed_constraints: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_constraint \
                 WHERE conname = 'compat_check_lifecycle_renamed'",
                &[],
            )
            .await
            .expect("check that the rejected rename published no new name")
            .get(0);

        // Assert
        assert_eq!(db_error(&rename_error).code().code(), "0A000");
        assert_eq!(original_constraints, 1);
        assert_eq!(renamed_constraints, 0);

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
        // Act
        let (
            initially_unenforced,
            failed_enable,
            state_after_failed_enable,
            enforced_state,
            rejected_new_violation,
            final_state,
            remaining,
        ) = run_foreign_key_enforcement_lifecycle(&client).await;

        // Assert
        assert_foreign_key_enforcement_lifecycle(
            &initially_unenforced,
            &failed_enable,
            &state_after_failed_enable,
            &enforced_state,
            &rejected_new_violation,
            &final_state,
            remaining,
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_validate_not_valid_foreign_key_after_repairing_existing_rows() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("validate_repaired_not_valid_fk").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_repaired_fk_parent (id INT PRIMARY KEY)",
                "CREATE TABLE compat_repaired_fk_child (parent_id INT)",
                "INSERT INTO compat_repaired_fk_child VALUES (9)",
                "ALTER TABLE compat_repaired_fk_child ADD CONSTRAINT \
                   compat_repaired_fk_name FOREIGN KEY (parent_id) \
                   REFERENCES compat_repaired_fk_parent (id) NOT VALID",
            ],
        )
        .await
        .expect("add a NOT VALID foreign key over the existing dangling row");
        let initially_validated: bool = client
            .query_one(
                "SELECT convalidated FROM pg_catalog.pg_constraint \
                 WHERE conname = 'compat_repaired_fk_name'",
                &[],
            )
            .await
            .expect("inspect the initial foreign-key validation state")
            .try_get(0)
            .expect("validation state");

        // Act
        client
            .execute("INSERT INTO compat_repaired_fk_parent VALUES (9)", &[])
            .await
            .expect("repair the existing dangling reference");
        client
            .batch_execute(
                "ALTER TABLE compat_repaired_fk_child \
                 VALIDATE CONSTRAINT compat_repaired_fk_name",
            )
            .await
            .expect("validate the foreign key after repairing all existing rows");
        let validated: bool = client
            .query_one(
                "SELECT convalidated FROM pg_catalog.pg_constraint \
                 WHERE conname = 'compat_repaired_fk_name'",
                &[],
            )
            .await
            .expect("inspect the successful foreign-key validation state")
            .try_get(0)
            .expect("validation state");
        let child_parent_id: i32 = client
            .query_one("SELECT parent_id FROM compat_repaired_fk_child", &[])
            .await
            .expect("read the repaired child reference")
            .try_get(0)
            .expect("parent id");
        let rejected_new_violation = client
            .execute("INSERT INTO compat_repaired_fk_child VALUES (10)", &[])
            .await
            .expect_err("the validated foreign key must reject new dangling references");

        // Assert
        assert!(!initially_validated);
        assert!(validated);
        assert_eq!(child_parent_id, 9);
        assert_eq!(db_error(&rejected_new_violation).code().code(), "23503");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_distinguish_match_full_from_simple_for_partial_null_keys() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("match_full_simple_partial_null").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_match_modes_parent (left_id INT, right_id INT)",
                "CREATE UNIQUE INDEX compat_match_modes_parent_key \
                   ON compat_match_modes_parent (left_id, right_id)",
                "CREATE TABLE compat_match_full_child (left_id INT, right_id INT, \
                   CONSTRAINT compat_match_full_child_fk FOREIGN KEY (left_id, right_id) \
                   REFERENCES compat_match_modes_parent (left_id, right_id) MATCH FULL)",
                "CREATE TABLE compat_match_simple_child (left_id INT, right_id INT, \
                   CONSTRAINT compat_match_simple_child_fk FOREIGN KEY (left_id, right_id) \
                   REFERENCES compat_match_modes_parent (left_id, right_id) MATCH SIMPLE)",
                "INSERT INTO compat_match_modes_parent VALUES (1, 2)",
            ],
        )
        .await
        .expect("create both composite match modes and a matching parent key");

        // Act
        client
            .execute(
                "INSERT INTO compat_match_simple_child VALUES (1, NULL)",
                &[],
            )
            .await
            .expect("MATCH SIMPLE skips checking a key containing any NULL field");
        let full_partial_null = client
            .execute("INSERT INTO compat_match_full_child VALUES (1, NULL)", &[])
            .await
            .expect_err("MATCH FULL rejects mixed NULL and non-NULL key fields");
        let accepted_simple_rows: i64 = client
            .query_one("SELECT count(*) FROM compat_match_simple_child", &[])
            .await
            .expect("count accepted MATCH SIMPLE rows")
            .try_get(0)
            .expect("row count");
        let accepted_full_rows: i64 = client
            .query_one("SELECT count(*) FROM compat_match_full_child", &[])
            .await
            .expect("count accepted MATCH FULL rows")
            .try_get(0)
            .expect("row count");

        // Assert
        assert_eq!(db_error(&full_partial_null).code().code(), "23503");
        assert_eq!(accepted_simple_rows, 1);
        assert_eq!(accepted_full_rows, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}
