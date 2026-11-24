use super::*;

#[test]
fn should_reindex_materialized_output_without_refreshing_rows() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("reindex_materialized_output").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE reindex_source(value INT)",
                "INSERT INTO reindex_source VALUES(7)",
                "CREATE MATERIALIZED VIEW reindex_view AS SELECT value FROM reindex_source",
                "CREATE UNIQUE INDEX reindex_idx ON reindex_view(value)",
                "REFRESH MATERIALIZED VIEW CONCURRENTLY reindex_view",
                "INSERT INTO reindex_source VALUES(11)",
            ],
        )
        .await
        .unwrap();
        let metadata = server
            .cassie
            .catalog
            .get_materialized_projection("postgres.public.reindex_view")
            .unwrap();
        let output = metadata.active_output_collection().unwrap();
        let before = crate::support_scalar_index_rebuild::remove_entries(&server.cassie, output);
        // Act
        client
            .batch_execute("REINDEX TABLE reindex_view")
            .await
            .unwrap();
        let rows = client
            .query("SELECT value FROM reindex_view", &[])
            .await
            .unwrap();
        // Assert
        assert_eq!(
            crate::support_scalar_index_rebuild::entries(&server.cassie, output),
            before
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get::<_, i32>(0), 7);
        let indexes = client
            .query(
                "SELECT tablename FROM pg_indexes WHERE indexname = 'reindex_idx'",
                &[],
            )
            .await
            .unwrap();
        assert_eq!(indexes.len(), 1);
        assert_eq!(indexes[0].get::<_, String>(0), "reindex_view");
        drop(client);
        server.shutdown(connection).await;
    });
}
