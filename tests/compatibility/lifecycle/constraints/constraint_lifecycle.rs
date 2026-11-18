use super::*;

#[path = "extended_constraint_forms.rs"]
mod extended_constraint_forms;

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
fn should_enforce_compact_column_check() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("compact_column_check").await;
        let (client, connection) = server.connect().await;

        // Act
        client
            .batch_execute("CREATE TABLE compat_compact_column (n INT CHECK(n>0))")
            .await
            .expect("create compact CHECK");
        client
            .batch_execute("INSERT INTO compat_compact_column (n) VALUES (1), (NULL)")
            .await
            .expect("allow true and UNKNOWN");
        let violation = client
            .batch_execute("INSERT INTO compat_compact_column (n) VALUES (0)")
            .await
            .expect_err("reject false");

        // Assert
        assert_eq!(db_error(&violation).code().code(), "23514");
        let count: i64 = client
            .query_one("SELECT COUNT(*) FROM compat_compact_column", &[])
            .await
            .expect("read after rejected insert")
            .get(0);
        assert_eq!(count, 2);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_enforce_compact_comparison_in_table_check() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("compact_table_check").await;
        let (client, connection) = server.connect().await;

        // Act
        client
            .batch_execute("CREATE TABLE compat_compact_table (n INT, CHECK(n>=0))")
            .await
            .expect("create compact table CHECK");
        let violation = client
            .batch_execute("INSERT INTO compat_compact_table (n) VALUES (-1)")
            .await
            .expect_err("reject false");

        // Assert
        assert_eq!(db_error(&violation).code().code(), "23514");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_keep_operator_text_inside_check_literal() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("compact_check_literal").await;
        let (client, connection) = server.connect().await;

        // Act
        client
            .batch_execute(
                "CREATE TABLE compat_compact_literal (label TEXT CHECK (label<>'it''s >= x'))",
            )
            .await
            .expect("create CHECK with operator text in literal");
        client
            .batch_execute("INSERT INTO compat_compact_literal (label) VALUES ('ok')")
            .await
            .expect("allow a different label");
        let violation = client
            .batch_execute("INSERT INTO compat_compact_literal (label) VALUES ('it''s >= x')")
            .await
            .expect_err("reject the exact literal");

        // Assert
        assert_eq!(db_error(&violation).code().code(), "23514");

        drop(client);
        server.shutdown(connection).await;
    });
}
