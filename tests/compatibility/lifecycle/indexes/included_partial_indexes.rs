use super::*;

#[test]
fn should_publish_partial_btree_index_clause_metadata() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("partial_index_include_metadata").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_index_include_rows \
                   (key_value INT, payload TEXT, active BOOLEAN)",
                "INSERT INTO compat_index_include_rows VALUES \
                   (3, 'high', TRUE), (2, 'filtered', FALSE), \
                   (1, 'low', TRUE), (NULL, 'null-key', TRUE)",
                "CREATE INDEX compat_index_include_idx \
                   ON compat_index_include_rows USING btree \
                   (key_value DESC NULLS LAST) INCLUDE (payload) WHERE active",
            ],
        )
        .await
        .expect("create an ordered partial btree index with an included column");

        // Act
        let metadata = client
            .query_one(
                "SELECT index_row.indnatts::INT, index_row.indnkeyatts::INT, \
                        index_row.indkey::text, \
                        index_row.indisvalid, index_row.indisready, \
                        pg_catalog.pg_get_expr(index_row.indpred, index_row.indrelid), \
                        pg_catalog.pg_get_indexdef(index_row.indexrelid) \
                 FROM pg_catalog.pg_index AS index_row \
                 WHERE index_row.indexrelid = \
                   'compat_index_include_idx'",
                &[],
            )
            .await
            .expect("read index key, included-column, and predicate metadata");
        let rows = client
            .query(
                "SELECT key_value, payload FROM compat_index_include_rows \
                 WHERE active ORDER BY key_value DESC NULLS LAST",
                &[],
            )
            .await
            .expect("read rows using the same predicate and ordering as the index");

        // Assert
        assert_eq!(
            metadata
                .columns()
                .iter()
                .map(|column| column.type_().oid())
                .collect::<Vec<_>>(),
            vec![23, 23, 25, 16, 16, 25, 25]
        );
        assert_eq!(metadata.get::<_, i32>(0), 2);
        assert_eq!(metadata.get::<_, i32>(1), 1);
        assert_eq!(metadata.get::<_, String>(2), "1 2");
        assert!(metadata.get::<_, bool>(3));
        assert!(metadata.get::<_, bool>(4));
        assert_eq!(metadata.get::<_, String>(5), "active");
        assert_eq!(
            metadata.get::<_, String>(6),
            "CREATE INDEX compat_index_include_idx ON public.compat_index_include_rows \
             USING btree (key_value DESC NULLS LAST) INCLUDE (payload) WHERE active"
        );
        assert_eq!(
            rows[0]
                .columns()
                .iter()
                .map(|column| column.type_().oid())
                .collect::<Vec<_>>(),
            vec![23, 25]
        );
        assert_eq!(
            rows.iter()
                .map(|row| (row.get::<_, Option<i32>>(0), row.get::<_, String>(1),))
                .collect::<Vec<_>>(),
            vec![
                (Some(3), "high".to_owned()),
                (Some(1), "low".to_owned()),
                (None, "null-key".to_owned()),
            ]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_bound_tenant_order_reads_only_when_the_partial_predicate_is_proven() {
    // Arrange
    current_thread_runtime().block_on(async {
        let server = CompatibilityServer::start("ordered_partial_membership").await;
        let (client, connection) = server.connect().await;
        execute_statements(&client, &[
            "CREATE TABLE orders (id INT PRIMARY KEY, tenant INT NOT NULL, status TEXT NOT NULL, created INT NOT NULL, payload TEXT)",
            "INSERT INTO orders VALUES (1,7,'open',2,'older'),(2,7,'open',2,'newer'),(3,7,'closed',3,'closed'),(4,8,'open',4,'other')",
            "CREATE INDEX open_orders ON orders(tenant, created DESC, id DESC) INCLUDE (payload) WHERE status='open'",
        ]).await.expect("seed orders with a tie, a closed order and another tenant");
        let query = "SELECT id,payload FROM orders WHERE tenant=7 AND status='open' ORDER BY created DESC,id DESC LIMIT 1";
        let closed = query.replace(" AND status='open'", "");
        let residual = query.replace("status='open'", "status='open' AND payload<>'newer'");

        // Act
        let plan: String = client.query_one(&format!("EXPLAIN {query}"), &[]).await.unwrap().get(0);
        let closed_plan: String = client.query_one(&format!("EXPLAIN {closed}"), &[]).await.unwrap().get(0);
        let first = client.query_one(query, &[]).await.unwrap();
        let inclusive = client.query_one(&closed, &[]).await.unwrap();
        let filtered = client.query_one(&residual, &[]).await.unwrap();

        // Assert
        assert!(plan.contains("access_path=ordered_bounded_scan") && plan.contains("index=open_orders"), "{plan}");
        assert!(!closed_plan.contains("index=open_orders"), "{closed_plan}");
        assert_eq!((first.get::<_, i32>(0), first.get::<_, String>(1)), (2, "newer".into()));
        assert_eq!(inclusive.get::<_, i32>(0), 3);
        assert_eq!(filtered.get::<_, i32>(0), 1);
        drop(client);
        server.shutdown(connection).await;
    });
}
