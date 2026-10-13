use super::*;

#[test]
fn should_report_descending_btree_key_order() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("descending_index_catalog").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_descending_index_rows (id INT, score INT)",
                "INSERT INTO compat_descending_index_rows VALUES (1, 10), (2, 20)",
                "CREATE INDEX compat_descending_index_idx \
                   ON compat_descending_index_rows USING btree (score DESC)",
            ],
        )
        .await
        .expect("create a descending btree key");

        // Act
        let index_definition: String = client
            .query_one(
                "SELECT indexdef FROM pg_catalog.pg_indexes \
                 WHERE tablename = 'compat_descending_index_rows' \
                   AND indexname = 'compat_descending_index_idx'",
                &[],
            )
            .await
            .expect("read the PostgreSQL index definition")
            .try_get(0)
            .expect("index definition");

        // Assert
        assert!(
            index_definition.contains("USING btree (score DESC)"),
            "PostgreSQL reports the method and key ordering; got {index_definition:?}"
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
