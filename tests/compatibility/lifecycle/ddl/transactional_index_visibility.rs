use super::*;

fn assert_index_rollback_observations(
    observations: Option<(i64, i64, i64, Vec<tokio_postgres::Row>)>,
) {
    let (owner_index_count, observer_before, observer_after, rows) =
        observations.expect("collect the index publication observations");
    assert_eq!(owner_index_count, 1);
    assert_eq!(observer_before, 0);
    assert_eq!(observer_after, 0);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].get::<_, i32>(0), 1);
    assert_eq!(rows[0].get::<_, String>(1), "alpha");
    assert_eq!(rows[1].get::<_, i32>(0), 2);
    assert_eq!(rows[1].get::<_, String>(1), "beta");
}

#[test]
fn should_hide_transactional_index_after_rollback() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("transactional_index_visibility").await;
        let (owner, owner_connection) = server.connect().await;
        let (observer, observer_connection) = server.connect().await;
        execute_statements(
            &owner,
            &[
                "CREATE TABLE compat_transactional_index_rows \
                 (id INT PRIMARY KEY, value TEXT)",
                "INSERT INTO compat_transactional_index_rows \
                 VALUES (1, 'alpha'), (2, 'beta')",
            ],
        )
        .await
        .expect("create and seed rows before the transactional index change");
        owner
            .batch_execute("BEGIN")
            .await
            .expect("begin the index publication transaction");

        // Act
        let create_index = owner
            .batch_execute(
                "CREATE INDEX compat_transactional_index_value_idx \
                 ON compat_transactional_index_rows (value)",
            )
            .await;
        let observations = if create_index.is_ok() {
            let owner_index_count: i64 = owner
                .query_one(
                    "SELECT count(*) FROM pg_catalog.pg_class \
                     WHERE relname = 'compat_transactional_index_value_idx' \
                       AND relkind = 'i'",
                    &[],
                )
                .await
                .expect("inspect the uncommitted index in the creating session")
                .get(0);
            let observer_index_count_before_rollback: i64 = observer
                .query_one(
                    "SELECT count(*) FROM pg_catalog.pg_class \
                     WHERE relname = 'compat_transactional_index_value_idx' \
                       AND relkind = 'i'",
                    &[],
                )
                .await
                .expect("inspect index visibility from an independent session")
                .get(0);
            owner
                .batch_execute("ROLLBACK")
                .await
                .expect("roll back the uncommitted index publication");
            let observer_index_count_after_rollback: i64 = observer
                .query_one(
                    "SELECT count(*) FROM pg_catalog.pg_class \
                     WHERE relname = 'compat_transactional_index_value_idx' \
                       AND relkind = 'i'",
                    &[],
                )
                .await
                .expect("confirm the rolled-back index is absent from the catalog")
                .get(0);
            let rows = observer
                .query(
                    "SELECT id, value FROM compat_transactional_index_rows ORDER BY id",
                    &[],
                )
                .await
                .expect("confirm rollback preserved the original table rows");
            Some((
                owner_index_count,
                observer_index_count_before_rollback,
                observer_index_count_after_rollback,
                rows,
            ))
        } else {
            owner
                .batch_execute("ROLLBACK")
                .await
                .expect("recover the owner after rejected index DDL");
            None
        };

        // Assert
        drop((owner, observer));
        server.shutdown(owner_connection).await;
        observer_connection.abort();
        let _ = observer_connection.await;
        create_index.expect("PostgreSQL accepts transactional CREATE INDEX");
        assert_index_rollback_observations(observations);
    });
}

#[test]
fn should_reuse_index_name_after_failed_build_savepoint_rollback() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("transactional_index_failure_recovery").await;
        let (owner, owner_connection) = server.connect().await;
        let (observer, observer_connection) = server.connect().await;
        execute_statements(
            &owner,
            &[
                "CREATE TABLE compat_failed_index_recovery_rows \
                   (id INT PRIMARY KEY, bucket INT)",
                "INSERT INTO compat_failed_index_recovery_rows VALUES (1, 4), (2, 4)",
            ],
        )
        .await
        .expect("create duplicate index keys before the failed index build");
        owner
            .batch_execute("BEGIN; SAVEPOINT before_index_build")
            .await
            .expect("begin a transaction and savepoint for index publication");

        // Act
        let failed_unique_index = owner
            .batch_execute(
                "CREATE UNIQUE INDEX compat_failed_index_recovery_idx \
                 ON compat_failed_index_recovery_rows (bucket)",
            )
            .await;
        let failed_build_sqlstate = failed_unique_index
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code().to_owned());
        owner
            .batch_execute("ROLLBACK TO SAVEPOINT before_index_build")
            .await
            .expect("recover the transaction after the duplicate-key index build");
        owner
            .batch_execute(
                "CREATE INDEX compat_failed_index_recovery_idx \
                 ON compat_failed_index_recovery_rows (bucket)",
            )
            .await
            .expect("reuse the failed index name for a nonunique index");
        owner
            .batch_execute("COMMIT")
            .await
            .expect("commit the recovered index publication");
        let published_index = observer
            .query_one(
                "SELECT index_entry.indisunique, relation.relkind::TEXT \
                 FROM pg_catalog.pg_index AS index_entry \
                 JOIN pg_catalog.pg_class AS relation \
                   ON relation.oid = index_entry.indexrelid \
                 WHERE relation.relname = 'compat_failed_index_recovery_idx'",
                &[],
            )
            .await
            .expect("inspect the recovered index from an independent session");
        let rows = observer
            .query(
                "SELECT id, bucket FROM compat_failed_index_recovery_rows ORDER BY id",
                &[],
            )
            .await
            .expect("confirm the failed build and recovery preserved source rows");

        // Assert
        drop((owner, observer));
        server.shutdown(owner_connection).await;
        observer_connection.abort();
        let _ = observer_connection.await;
        assert_eq!(failed_build_sqlstate.as_deref(), Some("23505"));
        assert!(!published_index.get::<_, bool>(0));
        assert_eq!(published_index.get::<_, String>(1), "i");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].get::<_, i32>(0), 1);
        assert_eq!(rows[0].get::<_, i32>(1), 4);
        assert_eq!(rows[1].get::<_, i32>(0), 2);
        assert_eq!(rows[1].get::<_, i32>(1), 4);
    });
}
