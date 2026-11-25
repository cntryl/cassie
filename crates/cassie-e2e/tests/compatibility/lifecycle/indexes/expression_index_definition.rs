use super::*;

#[test]
fn should_render_supported_expression_index_definitions() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("expression_index_definition").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_expression_index_rows (amount INT)",
                "CREATE INDEX compat_expression_index_idx \
                 ON compat_expression_index_rows ((amount::text))",
                "CREATE INDEX compat_case_expression_index_idx \
                 ON compat_expression_index_rows \
                 ((CASE WHEN amount > 0 THEN amount::text \
                        ELSE (-amount)::text END))",
            ],
        )
        .await
        .expect("create an index with a cast expression key");

        // Act
        let definition: String = client
            .query_one(
                "SELECT pg_catalog.pg_get_indexdef(index_row.indexrelid) \
                 FROM pg_catalog.pg_index AS index_row \
                 JOIN pg_catalog.pg_class AS index_class ON index_row.indexrelid = index_class.oid \
                 JOIN pg_catalog.pg_namespace AS namespace ON index_class.relnamespace = namespace.oid \
                 WHERE index_class.relname = 'compat_expression_index_idx' \
                   AND namespace.nspname = 'public'",
                &[],
            )
            .await
            .expect("read the cast expression index definition")
            .try_get(0)
            .expect("index definition");
        let case_definition: String = client
            .query_one(
                "SELECT pg_catalog.pg_get_indexdef(index_row.indexrelid) \
                 FROM pg_catalog.pg_index AS index_row \
                 JOIN pg_catalog.pg_class AS index_class ON index_row.indexrelid = index_class.oid \
                 JOIN pg_catalog.pg_namespace AS namespace ON index_class.relnamespace = namespace.oid \
                 WHERE index_class.relname = 'compat_case_expression_index_idx' \
                   AND namespace.nspname = 'public'",
                &[],
            )
            .await
            .expect("read the case expression index definition")
            .try_get(0)
            .expect("case index definition");
        // Assert
        assert_eq!(
            definition,
            "CREATE INDEX compat_expression_index_idx \
             ON public.compat_expression_index_rows USING btree ((amount)::text)"
        );
        assert_eq!(
            case_definition,
            "CREATE INDEX compat_case_expression_index_idx \
             ON public.compat_expression_index_rows USING btree \
             (CASE WHEN (amount > 0) THEN (amount)::text \
                   ELSE (-(amount))::text END)"
        );
        drop(client);
        server.shutdown(connection).await;
    });
}
