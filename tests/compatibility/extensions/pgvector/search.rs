use super::*;

#[test]
fn should_order_negative_inner_products_given_known_vectors_when_the_metric_operator_is_used() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime");
    runtime.block_on(async {
        let (server, client, connection, _) =
            start_vector_server("pgvector_negative_inner_product").await;
        for statement in [
            "CREATE TABLE compat_pgvector_inner_product \
             (id INT PRIMARY KEY, embedding vector(2))",
            "INSERT INTO compat_pgvector_inner_product VALUES \
             (1, '[1,0]'), (2, '[2,0]'), (3, '[0,1]')",
        ] {
            client
                .batch_execute(statement)
                .await
                .expect("create a small known vector corpus");
        }

        // Act
        let rows = client
            .query(
                "SELECT id, embedding <#> '[1,0]'::vector AS negative_inner_product \
                 FROM compat_pgvector_inner_product \
                 ORDER BY negative_inner_product, id",
                &[],
            )
            .await
            .expect("order rows using pgvector's negative inner-product operator");

        // Assert
        assert_eq!(
            rows.iter()
                .map(|row| (row.get::<_, i32>(0), row.get::<_, f64>(1)))
                .collect::<Vec<_>>(),
            vec![(2, -2.0), (1, -1.0), (3, 0.0)]
        );
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_match_exact_search_results_given_the_declared_ann_fixture_when_ann_search_executes() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime");
    runtime.block_on(async {
        let (server, client, connection, _) =
            start_vector_server("pgvector_ann_exact_results").await;
        for statement in [
            "CREATE TABLE compat_pgvector_ann_rows \
             (id INT PRIMARY KEY, embedding vector(2))",
            "INSERT INTO compat_pgvector_ann_rows VALUES \
             (1, '[1,0]'), (2, '[2,0]'), (3, '[3,0]'), (4, '[5,0]')",
            "CREATE INDEX compat_pgvector_ann_idx \
             ON compat_pgvector_ann_rows USING hnsw (embedding vector_l2_ops)",
        ] {
            client
                .batch_execute(statement)
                .await
                .expect("create an HNSW index over the exact-search fixture");
        }

        // Act
        let rows = client
            .query(
                "SELECT id, embedding <-> '[0,0]'::vector AS distance \
                 FROM compat_pgvector_ann_rows \
                 ORDER BY embedding <-> '[0,0]'::vector, id LIMIT 3",
                &[],
            )
            .await
            .expect("run exact nearest-neighbor search over the declared ANN fixture");

        // Assert
        assert_eq!(
            rows.iter()
                .map(|row| (row.get::<_, i32>(0), row.get::<_, f64>(1)))
                .collect::<Vec<_>>(),
            vec![(1, 1.0), (2, 2.0), (3, 3.0)]
        );
        drop(client);
        server.shutdown(connection).await;
    });
}
