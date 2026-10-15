use super::*;

#[test]
fn should_not_resolve_missing_schema_qualified_index_in_another_schema() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("drop_index_if_exists_scope").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_index_drop_target",
                "CREATE SCHEMA compat_index_drop_neighbor",
                "CREATE TABLE compat_index_drop_neighbor.rows (id INT)",
                "CREATE INDEX shared_idx ON compat_index_drop_neighbor.rows (id)",
            ],
        )
        .await
        .expect("create a same-named index only in a neighboring schema");

        // Act
        client
            .batch_execute("DROP INDEX IF EXISTS compat_index_drop_target.shared_idx")
            .await
            .expect("a missing qualified index should be ignored");
        let remaining = client
            .query(
                "SELECT schemaname, indexname, tablename \
                 FROM pg_catalog.pg_indexes \
                 WHERE indexname = 'shared_idx' \
                   AND schemaname IN (\
                     'compat_index_drop_target', 'compat_index_drop_neighbor') \
                 ORDER BY schemaname",
                &[],
            )
            .await
            .expect("inspect same-named indexes after the qualified no-op");

        // Assert
        let remaining = remaining
            .iter()
            .map(|row| {
                (
                    row.get::<_, String>(0),
                    row.get::<_, String>(1),
                    row.get::<_, String>(2),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            remaining,
            vec![(
                "compat_index_drop_neighbor".to_owned(),
                "shared_idx".to_owned(),
                "rows".to_owned(),
            )]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
