use super::*;

#[test]
fn should_create_table_as_an_existing_table_with_column_names() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("ctas_as_table_column_names").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_ctas_table_source (id INT, label TEXT NOT NULL)",
                "INSERT INTO compat_ctas_table_source VALUES (1, 'one'), (2, 'two')",
            ],
        )
        .await
        .expect("create and seed a CTAS table source");

        // Act
        let created = client
            .batch_execute(
                "CREATE TABLE compat_ctas_table_copy (copy_id, copy_label) \
                 AS TABLE compat_ctas_table_source",
            )
            .await;
        // Cassie exposes the implicit row id in `*`; project only the CTAS columns under test.
        let copied_rows = if created.is_ok() {
            Some(
                client
                    .query(
                        "SELECT copy_id, copy_label FROM compat_ctas_table_copy ORDER BY copy_id",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert!(
            created.is_ok(),
            "CREATE TABLE AS TABLE with an explicit column-name list should succeed: {created:?}"
        );
        let copied_rows = copied_rows
            .expect("successful CTAS should be queryable")
            .expect("read copied table rows");
        assert_eq!(copied_rows.len(), 2);
        assert_eq!(copied_rows[0].columns()[0].name(), "copy_id");
        assert_eq!(copied_rows[0].columns()[0].type_().oid(), 23);
        assert_eq!(copied_rows[0].columns()[1].name(), "copy_label");
        assert_eq!(copied_rows[0].columns()[1].type_().oid(), 25);
        assert_eq!(copied_rows[0].get::<_, i32>(0), 1);
        assert_eq!(copied_rows[0].get::<_, String>(1), "one");
        assert_eq!(copied_rows[1].get::<_, i32>(0), 2);
        assert_eq!(copied_rows[1].get::<_, String>(1), "two");
    });
}

#[test]
fn should_roll_back_ctas_table_when_query_materialization_fails() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("ctas_materialization_rollback").await;
        let (client, connection) = server.connect().await;

        // Act
        let failed = client
            .batch_execute("CREATE TABLE compat_ctas_failed_result AS SELECT 1 / 0 AS value")
            .await;
        let catalog_count = client
            .query_one(
                "SELECT count(*) FROM pg_class \
                 WHERE relname = 'compat_ctas_failed_result'",
                &[],
            )
            .await
            .expect("check catalog after failed CTAS");

        // Assert
        let sqlstate = failed
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code());
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(sqlstate, Some("22012"));
        assert_eq!(catalog_count.get::<_, i64>(0), 0);
    });
}

#[test]
fn should_rollback_explicit_transaction_ctas_for_all_sessions() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("ctas_explicit_transaction_rollback").await;
        let (owner, owner_connection) = server.connect().await;
        let (observer, observer_connection) = server.connect().await;

        // Act
        let began = owner.batch_execute("BEGIN").await;
        began.as_ref().expect("BEGIN should succeed");
        let created = owner
            .batch_execute(
                "CREATE TABLE compat_ctas_transactional_result AS \
                 VALUES (1, 'one'), (2, 'two')",
            )
            .await;
        let owner_rows = if created.is_ok() {
            Some(
                owner
                    .query(
                        "SELECT column1, column2 FROM compat_ctas_transactional_result \
                         ORDER BY column1",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };
        let observer_before_rollback = if created.is_ok() {
            Some(
                observer
                    .query_one(
                        "SELECT count(*) FROM pg_class \
                         WHERE relname = 'compat_ctas_transactional_result'",
                        &[],
                    )
                    .await,
            )
        } else {
            None
        };
        let rolled_back = owner.batch_execute("ROLLBACK").await;
        let observer_after_rollback = observer
            .query_one(
                "SELECT count(*) FROM pg_class \
                 WHERE relname = 'compat_ctas_transactional_result'",
                &[],
            )
            .await;

        // Assert
        drop(owner);
        drop(observer);
        observer_connection.abort();
        server.shutdown(owner_connection).await;
        assert!(began.is_ok(), "BEGIN should succeed: {began:?}");
        assert!(created.is_ok(), "CTAS should succeed: {created:?}");
        let owner_rows = owner_rows
            .expect("read-your-writes should follow successful CTAS")
            .expect("read CTAS rows in the owning transaction");
        assert_eq!(owner_rows.len(), 2);
        assert_eq!(owner_rows[0].get::<_, i32>(0), 1);
        assert_eq!(owner_rows[1].get::<_, i32>(0), 2);
        let observer_before_rollback = observer_before_rollback
            .expect("observer inspection should follow successful CTAS")
            .expect("inspect catalog from another session before rollback");
        assert_eq!(observer_before_rollback.get::<_, i64>(0), 0);
        assert!(
            rolled_back.is_ok(),
            "ROLLBACK should succeed: {rolled_back:?}"
        );
        let observer_after_rollback =
            observer_after_rollback.expect("inspect catalog after rollback");
        assert_eq!(observer_after_rollback.get::<_, i64>(0), 0);
    });
}

#[test]
fn should_reject_unsupported_ctas_access_methods_without_creating_a_table() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("ctas_unsupported_access_method").await;
        let (client, connection) = server.connect().await;

        // Act
        let rejected = client
            .batch_execute("CREATE TABLE compat_ctas_columnar USING columnar AS VALUES (1)")
            .await;
        let relation_count = client
            .query_one(
                "SELECT count(*) FROM pg_class WHERE relname = 'compat_ctas_columnar'",
                &[],
            )
            .await
            .expect("inspect target after rejecting unsupported CTAS access method");

        // Assert
        let sqlstate = rejected
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code());
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(sqlstate, Some("0A000"));
        assert_eq!(relation_count.get::<_, i64>(0), 0);
    });
}
