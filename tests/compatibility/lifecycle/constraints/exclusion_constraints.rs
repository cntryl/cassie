use super::*;

#[path = "constraint_control.rs"]
mod constraint_control;
#[path = "constraint_lifecycle.rs"]
mod constraint_lifecycle;
#[path = "constraint.rs"]
mod postgresql18_constraint_contract_wave;

#[test]
fn should_reject_not_valid_check_until_supported() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("not_valid_check_constraint").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_not_valid_check (id INT PRIMARY KEY, score INT)",
                "INSERT INTO compat_not_valid_check VALUES (1, -1)",
            ],
        )
        .await
        .expect("create a table with existing data before the unsupported constraint form");

        // Act
        let alter_error = client
            .batch_execute(
                "ALTER TABLE compat_not_valid_check ADD CONSTRAINT compat_nonnegative_score \
                 CHECK (score >= 0) NOT VALID",
            )
            .await
            .expect_err("NOT VALID CHECK constraints are outside the supported profile");
        let constraint_count: i64 = client
            .query_one(
                "SELECT count(*) FROM pg_catalog.pg_constraint \
                 WHERE conname = 'compat_nonnegative_score'",
                &[],
            )
            .await
            .expect("check that the rejected constraint was not published")
            .try_get(0)
            .expect("constraint count");
        let rows: i64 = client
            .query_one("SELECT count(*) FROM compat_not_valid_check", &[])
            .await
            .expect("inspect source rows after the rejected ALTER")
            .try_get(0)
            .expect("row count");

        // Assert
        assert_eq!(db_error(&alter_error).code().code(), "0A000");
        assert_eq!(constraint_count, 0);
        assert_eq!(rows, 1);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_not_enforced_check_until_supported() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("not_enforced_check_constraint").await;
        let (client, connection) = server.connect().await;
        let create_error = client
            .batch_execute(
                "CREATE TABLE compat_not_enforced_check (value INT, \
                 CONSTRAINT compat_positive_not_enforced CHECK (value > 0) NOT ENFORCED)",
            )
            .await
            .expect_err("NOT ENFORCED CHECK constraints are outside the supported profile");

        // Act
        let relation_exists: bool = client
            .query_one(
                "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
                 WHERE table_name = 'compat_not_enforced_check')",
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

#[test]
fn should_reject_check_predicates_comparing_multiple_columns_until_supported() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("multicolumn_check_constraint").await;
        let (client, connection) = server.connect().await;
        let create_error = client
            .batch_execute(
                "CREATE TABLE compat_multicolumn_check (left_value INT, right_value INT, \
                   CONSTRAINT compat_column_order CHECK (left_value > right_value))",
            )
            .await
            .expect_err("general multi-column CHECK expressions remain unsupported");

        // Act
        let relation_exists: bool = client
            .query_one(
                "SELECT EXISTS (SELECT 1 FROM information_schema.tables \
                 WHERE table_name = 'compat_multicolumn_check')",
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
