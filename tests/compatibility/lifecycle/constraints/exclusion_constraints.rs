use super::*;

#[path = "constraint_control.rs"]
mod constraint_control;
#[path = "constraint_lifecycle.rs"]
mod constraint_lifecycle;
#[path = "foreign_key_actions.rs"]
mod foreign_key_actions;
#[path = "constraint.rs"]
mod postgresql18_constraint_contract_wave;
#[path = "temporal_period_catalog.rs"]
mod temporal_period_catalog;

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
fn should_enforce_match_full_for_composite_foreign_keys() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("match_full_foreign_key").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_match_full_parent (left_id INT, right_id INT)",
                "CREATE UNIQUE INDEX compat_match_full_parent_key \
                   ON compat_match_full_parent (left_id, right_id)",
                "CREATE TABLE compat_match_full_child (left_id INT, right_id INT, \
                   CONSTRAINT compat_match_full_fk FOREIGN KEY (left_id, right_id) \
                   REFERENCES compat_match_full_parent (left_id, right_id) MATCH FULL)",
                "INSERT INTO compat_match_full_parent VALUES (1, 2)",
                "INSERT INTO compat_match_full_child VALUES (NULL, NULL)",
            ],
        )
        .await
        .expect("create the composite foreign key and accept a fully NULL key");

        // Act
        let partial_null = client
            .execute(
                "INSERT INTO compat_match_full_child VALUES (1, NULL)",
                &[],
            )
            .await
            .expect_err("MATCH FULL must reject a key that mixes NULL and non-NULL fields");
        let accepted_rows = client
            .query(
                "SELECT left_id, right_id FROM compat_match_full_child ORDER BY left_id NULLS FIRST",
                &[],
            )
            .await
            .expect("read the row accepted before the rejected insert");

        // Assert
        assert_eq!(db_error(&partial_null).code().code(), "23503");
        assert_eq!(accepted_rows.len(), 1);
        assert_eq!(accepted_rows[0].get::<_, Option<i32>>(0), None);
        assert_eq!(accepted_rows[0].get::<_, Option<i32>>(1), None);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_validate_not_valid_foreign_key_without_accepting_new_violations() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("not_valid_foreign_key").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_not_valid_fk_parent (id INT PRIMARY KEY)",
                "CREATE TABLE compat_not_valid_fk_child (parent_id INT)",
                "INSERT INTO compat_not_valid_fk_parent VALUES (1)",
                "INSERT INTO compat_not_valid_fk_child VALUES (9)",
                "ALTER TABLE compat_not_valid_fk_child \
                   ADD CONSTRAINT compat_not_valid_fk \
                   FOREIGN KEY (parent_id) \
                   REFERENCES compat_not_valid_fk_parent(id) NOT VALID",
                "BEGIN",
                "SAVEPOINT before_new_fk_violation",
            ],
        )
        .await
        .expect("add a not-valid foreign key over a preexisting violation");

        // Act
        let new_violation = client
            .execute("INSERT INTO compat_not_valid_fk_child VALUES (10)", &[])
            .await
            .expect_err("NOT VALID must still reject new foreign-key violations");
        client
            .batch_execute("ROLLBACK TO SAVEPOINT before_new_fk_violation")
            .await
            .expect("restore the transaction after the rejected new row");
        client
            .batch_execute("SAVEPOINT before_fk_validation")
            .await
            .expect("save the transaction before validating old rows");
        let validation = client
            .batch_execute(
                "ALTER TABLE compat_not_valid_fk_child \
                 VALIDATE CONSTRAINT compat_not_valid_fk",
            )
            .await
            .expect_err("validation must find the preexisting foreign-key violation");
        client
            .batch_execute("ROLLBACK TO SAVEPOINT before_fk_validation")
            .await
            .expect("restore the transaction after failed validation");
        let validated: bool = client
            .query_one(
                "SELECT convalidated FROM pg_constraint \
                 WHERE conname = 'compat_not_valid_fk'",
                &[],
            )
            .await
            .expect("inspect the validation flag")
            .try_get(0)
            .expect("constraint validation state");
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("finish the constraint-validation transaction");

        // Assert
        assert_eq!(db_error(&new_violation).code().code(), "23503");
        assert_eq!(db_error(&validation).code().code(), "23503");
        assert!(!validated);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_expose_named_not_null_constraints_in_pg_constraint() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("named_not_null_constraint").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_named_not_null (id INT \
                 CONSTRAINT compat_named_id_nn NOT NULL)",
            )
            .await
            .expect("create a named NOT NULL constraint");

        // Act
        let metadata = client
            .query_opt(
                "SELECT conname, contype::TEXT FROM pg_constraint \
                 WHERE conname = 'compat_named_id_nn'",
                &[],
            )
            .await
            .expect("read named NOT NULL constraint metadata");

        // Assert
        let metadata = metadata.expect("PostgreSQL 18 records named NOT NULL constraints");
        assert_eq!(metadata.get::<_, String>(0), "compat_named_id_nn");
        assert_eq!(metadata.get::<_, String>(1), "n");

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
