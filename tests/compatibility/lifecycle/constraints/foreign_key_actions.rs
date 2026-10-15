use super::*;

#[test]
fn should_apply_on_delete_set_null_column_list() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("foreign_key_set_null_column_list").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_fk_set_null_parent (id INT PRIMARY KEY)",
                "INSERT INTO compat_fk_set_null_parent VALUES (1)",
            ],
        )
        .await
        .expect("create and seed the composite foreign-key target");

        // Act
        let child_created = client
            .batch_execute(
                "CREATE TABLE compat_fk_set_null_child (row_id INT PRIMARY KEY, \
                   parent_id INT, FOREIGN KEY (parent_id) \
                   REFERENCES compat_fk_set_null_parent (id) \
                   ON DELETE SET NULL (parent_id))",
            )
            .await;
        let (inserted, deleted, remaining) = if child_created.is_ok() {
            let inserted = client
                .execute("INSERT INTO compat_fk_set_null_child VALUES (1, 1)", &[])
                .await;
            let deleted = if inserted.is_ok() {
                Some(
                    client
                        .execute(
                            "DELETE FROM compat_fk_set_null_parent \
                         WHERE id = 1",
                            &[],
                        )
                        .await,
                )
            } else {
                None
            };
            let remaining = if deleted.as_ref().is_some_and(Result::is_ok) {
                Some(
                    client
                        .query(
                            "SELECT parent_id FROM compat_fk_set_null_child \
                             WHERE row_id = 1",
                            &[],
                        )
                        .await,
                )
            } else {
                None
            };
            (Some(inserted), deleted, remaining)
        } else {
            (None, None, None)
        };

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert!(
            child_created.is_ok(),
            "ON DELETE SET NULL with a column list should be accepted: {child_created:?}"
        );
        assert_eq!(
            inserted
                .expect("insert a child referencing the parent")
                .expect("the referenced child should be accepted"),
            1
        );
        assert_eq!(
            deleted
                .expect("delete the referenced parent")
                .expect("SET NULL should preserve the child row"),
            1
        );
        let row = remaining
            .expect("read the child after deleting its parent")
            .expect("the child row should remain after SET NULL");
        assert_eq!(row.len(), 1);
        assert_eq!(row[0].get::<_, Option<i32>>(0), None);
    });
}

#[test]
fn should_apply_on_delete_set_default_column_list() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("foreign_key_set_default_column_list").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_fk_set_default_parent (id INT PRIMARY KEY)",
                "INSERT INTO compat_fk_set_default_parent VALUES (2), (3)",
            ],
        )
        .await
        .expect("create and seed the composite foreign-key target");

        // Act
        let child_created = client
            .batch_execute(
                "CREATE TABLE compat_fk_set_default_child (row_id INT PRIMARY KEY, \
                   parent_id INT DEFAULT 3, FOREIGN KEY (parent_id) \
                   REFERENCES compat_fk_set_default_parent (id) \
                   ON DELETE SET DEFAULT (parent_id))",
            )
            .await;
        let (inserted, deleted, remaining) = if child_created.is_ok() {
            let inserted = client
                .execute("INSERT INTO compat_fk_set_default_child VALUES (1, 2)", &[])
                .await;
            let deleted = if inserted.is_ok() {
                Some(
                    client
                        .execute(
                            "DELETE FROM compat_fk_set_default_parent \
                         WHERE id = 2",
                            &[],
                        )
                        .await,
                )
            } else {
                None
            };
            let remaining = if deleted.as_ref().is_some_and(Result::is_ok) {
                Some(
                    client
                        .query(
                            "SELECT parent_id FROM compat_fk_set_default_child \
                             WHERE row_id = 1",
                            &[],
                        )
                        .await,
                )
            } else {
                None
            };
            (Some(inserted), deleted, remaining)
        } else {
            (None, None, None)
        };

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert!(
            child_created.is_ok(),
            "ON DELETE SET DEFAULT with a column list should be accepted: {child_created:?}"
        );
        assert_eq!(
            inserted
                .expect("insert a child referencing the parent")
                .expect("the referenced child should be accepted"),
            1
        );
        assert_eq!(
            deleted
                .expect("delete the referenced parent")
                .expect("SET DEFAULT should preserve the child row"),
            1
        );
        let row = remaining
            .expect("read the child after deleting its parent")
            .expect("the child row should remain after SET DEFAULT");
        assert_eq!(row.len(), 1);
        assert_eq!(row[0].get::<_, i32>(0), 3);
    });
}

#[test]
fn should_report_postgres_sqlstate_for_unsupported_match_partial() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("foreign_key_match_partial").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TABLE compat_fk_match_partial_parent (left_id INT, right_id INT, \
                   UNIQUE (left_id, right_id))",
            )
            .await
            .expect("create the referenced composite key");

        // Act
        let unsupported = client
            .batch_execute(
                "CREATE TABLE compat_fk_match_partial_child (left_id INT, right_id INT, \
                   FOREIGN KEY (left_id, right_id) \
                   REFERENCES compat_fk_match_partial_parent (left_id, right_id) \
                   MATCH PARTIAL)",
            )
            .await
            .expect_err("PostgreSQL accepts MATCH PARTIAL syntax but reports it as unimplemented");

        // Assert
        assert_eq!(db_error(&unsupported).code().code(), "0A000");

        drop(client);
        server.shutdown(connection).await;
    });
}
