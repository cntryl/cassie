use super::*;

#[test]
fn should_restore_existing_relation_identity_after_lifecycle_savepoint_rollback() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue_839_savepoint_identity").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_issue839_savepoint_rows \
                 (id INT PRIMARY KEY, amount INT NOT NULL)",
                "INSERT INTO compat_issue839_savepoint_rows VALUES (1, 41), (2, 42)",
            ],
        )
        .await
        .expect("create a populated relation with a primary-key index");
        let identity_before = client
            .query_one(
                "SELECT relation.oid::BIGINT, index_metadata.indexrelid::BIGINT \
                 FROM pg_catalog.pg_class AS relation \
                 JOIN pg_catalog.pg_namespace AS relation_schema \
                   ON relation_schema.oid = relation.relnamespace \
                 JOIN pg_catalog.pg_index AS index_metadata \
                   ON index_metadata.indrelid = relation.oid \
                 WHERE relation_schema.nspname = 'public' \
                   AND relation.relname = 'compat_issue839_savepoint_rows' \
                   AND index_metadata.indisprimary",
                &[],
            )
            .await
            .expect("capture relation and primary-key index identity before lifecycle DDL");

        // Act
        let rollback_result = execute_statements(
            &client,
            &[
                "BEGIN",
                "SAVEPOINT before_lifecycle",
                "ALTER TABLE compat_issue839_savepoint_rows \
                 RENAME TO compat_issue839_savepoint_renamed",
                "ALTER TABLE compat_issue839_savepoint_renamed \
                 ALTER COLUMN amount TYPE NUMERIC(20,0) USING amount::numeric, \
                 ADD COLUMN transient TEXT DEFAULT 'temporary'",
                "DROP TABLE compat_issue839_savepoint_renamed",
                "ROLLBACK TO SAVEPOINT before_lifecycle",
            ],
        )
        .await;
        let state_after_rollback = if rollback_result.is_ok() {
            let identity = client
                .query_one(
                    "SELECT relation.oid::BIGINT, index_metadata.indexrelid::BIGINT \
                     FROM pg_catalog.pg_class AS relation \
                     JOIN pg_catalog.pg_namespace AS relation_schema \
                       ON relation_schema.oid = relation.relnamespace \
                     JOIN pg_catalog.pg_index AS index_metadata \
                       ON index_metadata.indrelid = relation.oid \
                     WHERE relation_schema.nspname = 'public' \
                       AND relation.relname = 'compat_issue839_savepoint_rows' \
                       AND index_metadata.indisprimary",
                    &[],
                )
                .await;
            let metadata = client
                .query_one(
                    "SELECT attribute.atttypid::BIGINT, attribute.atttypmod, \
                     NOT EXISTS (SELECT 1 FROM pg_catalog.pg_attribute AS added \
                       WHERE added.attrelid = relation.oid \
                         AND added.attname = 'transient' AND NOT added.attisdropped), \
                     pg_catalog.to_regclass(\
                       'public.compat_issue839_savepoint_renamed') IS NULL \
                     FROM pg_catalog.pg_class AS relation \
                     JOIN pg_catalog.pg_namespace AS relation_schema \
                       ON relation_schema.oid = relation.relnamespace \
                     JOIN pg_catalog.pg_attribute AS attribute \
                       ON attribute.attrelid = relation.oid \
                     WHERE relation_schema.nspname = 'public' \
                       AND relation.relname = 'compat_issue839_savepoint_rows' \
                       AND attribute.attname = 'amount'",
                    &[],
                )
                .await;
            let rows = client
                .query(
                    "SELECT id, amount FROM compat_issue839_savepoint_rows ORDER BY id",
                    &[],
                )
                .await;
            Some((identity, metadata, rows))
        } else {
            None
        };
        let finish_result = if rollback_result.is_ok() {
            Some(
                execute_statements(
                    &client,
                    &[
                        "RELEASE SAVEPOINT before_lifecycle",
                        "ALTER TABLE compat_issue839_savepoint_rows \
                         ADD COLUMN committed_after_rollback INT DEFAULT 9",
                        "COMMIT",
                    ],
                )
                .await,
            )
        } else {
            None
        };
        let committed_state = if matches!(&finish_result, Some(Ok(()))) {
            let identity = client
                .query_one(
                    "SELECT relation.oid::BIGINT, index_metadata.indexrelid::BIGINT \
                     FROM pg_catalog.pg_class AS relation \
                     JOIN pg_catalog.pg_namespace AS relation_schema \
                       ON relation_schema.oid = relation.relnamespace \
                     JOIN pg_catalog.pg_index AS index_metadata \
                       ON index_metadata.indrelid = relation.oid \
                     WHERE relation_schema.nspname = 'public' \
                       AND relation.relname = 'compat_issue839_savepoint_rows' \
                       AND index_metadata.indisprimary",
                    &[],
                )
                .await;
            let rows = client
                .query(
                    "SELECT id, amount, committed_after_rollback \
                     FROM compat_issue839_savepoint_rows ORDER BY id",
                    &[],
                )
                .await;
            let default_value = client
                .query_one(
                    "SELECT column_default FROM information_schema.columns \
                     WHERE table_schema = 'public' \
                       AND table_name = 'compat_issue839_savepoint_rows' \
                       AND column_name = 'committed_after_rollback'",
                    &[],
                )
                .await;
            Some((identity, rows, default_value))
        } else {
            None
        };

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert!(
            rollback_result.is_ok(),
            "savepoint rollback should restore the original relation after rename, ALTER, and DROP: {rollback_result:?}"
        );
        let (identity_after, metadata_after, rows_after) = state_after_rollback
            .expect("inspect the restored relation after savepoint rollback");
        let identity_after = identity_after.expect("read relation and index identities after rollback");
        assert_eq!(
            (
                identity_before.get::<_, i64>(0),
                identity_before.get::<_, i64>(1),
            ),
            (
                identity_after.get::<_, i64>(0),
                identity_after.get::<_, i64>(1),
            ),
            "savepoint rollback must preserve the original relation and index OIDs"
        );
        let metadata_after = metadata_after.expect("read original column metadata after rollback");
        assert_eq!(metadata_after.get::<_, i64>(0), 23);
        assert_eq!(metadata_after.get::<_, i32>(1), -1);
        assert!(metadata_after.get::<_, bool>(2));
        assert!(metadata_after.get::<_, bool>(3));
        let rows_after = rows_after.expect("read original values after rollback");
        assert_eq!(rows_after.len(), 2);
        assert_eq!(
            rows_after
                .iter()
                .map(|row| (row.get::<_, i32>(0), row.get::<_, i32>(1)))
                .collect::<Vec<_>>(),
            vec![(1, 41), (2, 42)]
        );

        assert!(
            matches!(&finish_result, Some(Ok(()))),
            "the transaction should remain usable after savepoint rollback: {finish_result:?}"
        );
        let (identity_after_commit, committed_rows, default_value) = committed_state
            .expect("inspect DDL committed after the savepoint rollback");
        let identity_after_commit =
            identity_after_commit.expect("read relation identity after the outer commit");
        assert_eq!(
            (
                identity_before.get::<_, i64>(0),
                identity_before.get::<_, i64>(1),
            ),
            (
                identity_after_commit.get::<_, i64>(0),
                identity_after_commit.get::<_, i64>(1),
            )
        );
        let committed_rows = committed_rows.expect("read rows after the outer commit");
        assert_eq!(committed_rows.len(), 2);
        assert_eq!(
            committed_rows
                .iter()
                .map(|row| (
                    row.get::<_, i32>(0),
                    row.get::<_, i32>(1),
                    row.get::<_, i32>(2),
                ))
                .collect::<Vec<_>>(),
            vec![(1, 41, 9), (2, 42, 9)]
        );
        assert_eq!(
            default_value
                .expect("read the committed default")
                .get::<_, String>(0),
            "9"
        );
    });
}
