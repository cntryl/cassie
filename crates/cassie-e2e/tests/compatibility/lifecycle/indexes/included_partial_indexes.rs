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
                 JOIN pg_catalog.pg_class AS index_class ON index_row.indexrelid = index_class.oid \
                 JOIN pg_catalog.pg_namespace AS namespace ON index_class.relnamespace = namespace.oid \
                 WHERE index_class.relname = 'compat_index_include_idx' \
                   AND namespace.nspname = 'public'",
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
        assert!(plan.contains("access_path=ordered_bounded_scan") && plan.contains("open_orders"), "{plan}");
        assert!(!closed_plan.contains("open_orders"), "{closed_plan}");
        assert_eq!((first.get::<_, i32>(0), first.get::<_, String>(1)), (2, "newer".into()));
        assert_eq!(inclusive.get::<_, i32>(0), 3);
        assert_eq!(filtered.get::<_, i32>(0), 1);
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_count_only_joined_survivors_toward_the_ordered_limit() {
    // Arrange
    current_thread_runtime().block_on(async {
        let server = CompatibilityServer::start("ordered_join_survivors").await;
        let (client, connection) = server.connect().await;
        execute_statements(&client, &[
            "CREATE TABLE orders (id INT PRIMARY KEY, tenant INT NOT NULL, status TEXT NOT NULL, created INT NOT NULL, account_id INT, product_id INT, payload TEXT)",
            "CREATE TABLE accounts (id INT PRIMARY KEY, name TEXT)",
            "CREATE TABLE products (id INT PRIMARY KEY, price INT)",
            "INSERT INTO orders VALUES (1,7,'open',1,1,1,'first'),(2,7,'open',1,1,1,'second'),(3,7,'open',2,1,2,'rejected'),(4,7,'open',2,2,1,'missing'),(5,7,'closed',3,1,1,'closed')",
            "INSERT INTO accounts VALUES (1,'account')",
            "INSERT INTO products VALUES (1,10),(2,20)",
            "CREATE INDEX order_feed ON orders(tenant, created DESC, id DESC) INCLUDE (account_id,product_id,payload) WHERE status='open'",
        ]).await.expect("seed missing account, rejected product and closed order");
        let query = "SELECT orders.id,orders.payload,accounts.name,products.price FROM orders JOIN accounts ON orders.account_id=accounts.id JOIN products ON orders.product_id=products.id WHERE orders.tenant=7 AND orders.status='open' AND products.price<>20 ORDER BY orders.created DESC,orders.id DESC LIMIT 2";

        // Act
        let plan: String = client.query_one(&format!("EXPLAIN ANALYZE {query}"), &[]).await.unwrap().get(0);
        let rows = client.query(query, &[]).await.unwrap();
        client.batch_execute("DROP INDEX order_feed").await.unwrap();
        let scan = client.query(query, &[]).await.unwrap();

        // Assert
        for actual in ["join_strategy=ordered_index_nested_loop", "order_index_entries=4", "order_rows=4", "order_base_fetches=0", "dimension_probes=7", "dimension_base_fetches=6", "intermediate_rows=6", "joined_rows=2"] {
            assert!(plan.split_whitespace().any(|field| field == actual), "{actual} missing in {plan}");
        }
        for result in [rows, scan] {
            assert_eq!(result.iter().map(|r| (r.get::<_,i32>(0),r.get::<_,String>(1),r.get::<_,String>(2),r.get::<_,i32>(3))).collect::<Vec<_>>(), vec![(2,"second".into(),"account".into(),10),(1,"first".into(),"account".into(),10)]);
        }
        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_preserve_sql_order_given_noncanonical_partial_index_keys() {
    // Arrange
    current_thread_runtime().block_on(async {
        let server = CompatibilityServer::start("typed_partial_order").await;
        let (client, connection) = server.connect().await;
        execute_statements(&client, &[
            "CREATE TABLE typed_orders (id INT PRIMARY KEY, tenant INT NOT NULL, status TEXT NOT NULL, amount NUMERIC NOT NULL, instant TIMESTAMP NOT NULL)",
            "INSERT INTO typed_orders VALUES (1,7,'open',2,'9999-01-01'),(2,7,'open',10,'10000-01-01')",
            "CREATE INDEX decimal_feed ON typed_orders(tenant,amount,id) WHERE status='open'",
            "CREATE INDEX timestamp_feed ON typed_orders(tenant,instant,id) WHERE status='open'",
        ]).await.expect("create index values with reversed text and SQL order");

        // Act
        let mut ids = Vec::new();
        for column in ["amount", "instant"] {
            let sql = format!("SELECT id FROM typed_orders WHERE tenant=7 AND status='open' ORDER BY {column} DESC,id DESC LIMIT 1");
            ids.push(client.query_one(&sql, &[]).await.unwrap().get::<_,i32>(0));
        }

        // Assert
        assert_eq!(ids, [2,2]);
        drop(client);
        server.shutdown(connection).await;
    });
}
