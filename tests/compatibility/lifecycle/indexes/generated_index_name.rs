use super::*;

#[test]
fn should_generate_postgresql_name_for_an_unnamed_index() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("generated_index_name").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_generated_index_rows (customer_id INT)")
            .await
            .expect("create a table for an unnamed index");

        // Act
        client
            .batch_execute("CREATE INDEX ON compat_generated_index_rows (customer_id)")
            .await
            .expect("create an index without an explicit name");
        let index = client
            .query_one(
                "SELECT indexname, indexdef FROM pg_catalog.pg_indexes \
                 WHERE schemaname = 'public' \
                   AND tablename = 'compat_generated_index_rows'",
                &[],
            )
            .await
            .expect("read the generated index name and definition");

        // Assert
        assert_eq!(
            index
                .columns()
                .iter()
                .map(|column| column.type_().oid())
                .collect::<Vec<_>>(),
            vec![25, 25]
        );
        assert_eq!(
            index.get::<_, String>(0),
            "compat_generated_index_rows_customer_id_idx"
        );
        assert_eq!(
            index.get::<_, String>(1),
            "CREATE INDEX compat_generated_index_rows_customer_id_idx \
             ON public.compat_generated_index_rows USING btree (customer_id)"
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
