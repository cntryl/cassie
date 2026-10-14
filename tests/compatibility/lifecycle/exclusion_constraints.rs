use super::*;

#[test]
fn should_enforce_exclusion_predicate_report_sqlstate() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("partial_exclusion_constraint").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_exclusion_slots (slot INT NOT NULL, active BOOL NOT NULL, \
                   CONSTRAINT compat_active_slot_excl EXCLUDE USING btree (slot WITH =) WHERE (active))",
                "INSERT INTO compat_exclusion_slots VALUES (1, true), (1, false)",
            ],
        )
        .await
        .expect("create partial exclusion constraint and rows outside its predicate");

        // Act
        let conflict = client
            .execute("INSERT INTO compat_exclusion_slots VALUES (1, true)", &[])
            .await
            .expect_err("an active row must conflict with the existing active key");
        let rows = client
            .query(
                "SELECT active FROM compat_exclusion_slots ORDER BY active DESC",
                &[],
            )
            .await
            .expect("read rows after rejected exclusion conflict");

        // Assert
        assert_eq!(db_error(&conflict).code().code(), "23P01");
        assert_eq!(rows.len(), 2);
        assert!(rows[0].get::<_, bool>(0));
        assert!(!rows[1].get::<_, bool>(0));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_check_deferred_exclusion_conflicts_at_commit_rollback() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("deferred_exclusion_constraint").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_deferred_slots (slot INT NOT NULL, \
                   CONSTRAINT compat_deferred_slot_excl EXCLUDE USING btree (slot WITH =) \
                   DEFERRABLE INITIALLY DEFERRED)",
                "INSERT INTO compat_deferred_slots VALUES (1)",
            ],
        )
        .await
        .expect("create deferred exclusion constraint and initial row");
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin deferred exclusion transaction");

        // Act
        client
            .execute("INSERT INTO compat_deferred_slots VALUES (1)", &[])
            .await
            .expect("deferred conflict should wait until transaction completion");
        let commit = client
            .batch_execute("COMMIT")
            .await
            .expect_err("commit must reject the deferred exclusion conflict");
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("clear transaction state after rejected commit");
        let rows = client
            .query("SELECT slot FROM compat_deferred_slots", &[])
            .await
            .expect("read rows after rejected commit");

        // Assert
        assert_eq!(db_error(&commit).code().code(), "23P01");
        assert_eq!(rows.len(), 1, "the rejected transaction must be atomic");
        assert_eq!(rows[0].get::<_, i32>(0), 1);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_enforce_not_valid_check_on_new_rows_then_validate_existing_rows() {
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
                "ALTER TABLE compat_not_valid_check ADD CONSTRAINT compat_nonnegative_score \
                   CHECK (score >= 0) NOT VALID",
                "INSERT INTO compat_not_valid_check VALUES (2, 1)",
            ],
        )
        .await
        .expect("add an unvalidated check over existing data and accept valid new data");

        // Act
        let rejected_new_row = client
            .execute("INSERT INTO compat_not_valid_check VALUES (3, -2)", &[])
            .await
            .expect_err("NOT VALID must still enforce new rows");
        let rejected_validation = client
            .batch_execute(
                "ALTER TABLE compat_not_valid_check \
                 VALIDATE CONSTRAINT compat_nonnegative_score",
            )
            .await
            .expect_err("validation must find the preexisting violation");
        let constraint_state: bool = client
            .query_one(
                "SELECT convalidated FROM pg_catalog.pg_constraint \
                 WHERE conname = 'compat_nonnegative_score'",
                &[],
            )
            .await
            .expect("inspect validation state after failed validation")
            .try_get(0)
            .expect("constraint validation flag");
        let rows = client
            .query(
                "SELECT id, score FROM compat_not_valid_check ORDER BY id",
                &[],
            )
            .await
            .expect("read rows after rejected inserts and validation");

        // Assert
        assert_eq!(db_error(&rejected_new_row).code().code(), "23514");
        assert_eq!(db_error(&rejected_validation).code().code(), "23514");
        assert!(!constraint_state);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, i32>(1), -1);
        assert_eq!(rows[1].get::<_, i32>(0), 2);
        assert_eq!(rows[1].get::<_, i32>(1), 1);

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
