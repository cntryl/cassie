#[test]
fn should_lock_scalar_read_model_hot_path_access_paths() {
    // Arrange
    let path = data_dir("read_model_hot_paths");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        let session = cassie.create_session("tester", None);
        seed_read_model_hot_paths(&cassie, &session);
        let before = cassie.metrics();

        // Act
        let page = cassie
            .execute_sql(
                &session,
                "SELECT body FROM read_model_hot_paths \
                 WHERE tenant = 'tenant-a' AND status = 'open' AND created_at >= 10 \
                 ORDER BY status DESC, created_at ASC LIMIT 2",
                vec![],
            )
            .unwrap();
        let page_explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT body FROM read_model_hot_paths \
                 WHERE tenant = 'tenant-a' AND status = 'open' AND created_at >= 10 \
                 ORDER BY status DESC, created_at ASC LIMIT 2",
                vec![],
            )
            .unwrap();
        let expression = cassie
            .execute_sql(
                &session,
                "SELECT body FROM read_model_hot_paths WHERE lower(title) = 'alpha'",
                vec![],
            )
            .unwrap();
        let expression_explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT body FROM read_model_hot_paths WHERE lower(title) = 'alpha'",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_hot_path_results(&page, &expression);
        assert_hot_path_plans(&page_explain, &expression_explain);
        assert_hot_path_metrics(&before, &after);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_lock_column_batch_covered_read_hot_path() {
    // Arrange
    let path = data_dir("read_model_column_batch");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE read_model_column_batch \
                 (title TEXT, body TEXT, status TEXT, score INT)",
                vec![],
            )
            .unwrap();
        for (title, body, status, score) in [
            ("alpha", "one", "approved", 10),
            ("beta", "two", "pending", 20),
            ("gamma", "three", "approved", 30),
        ] {
            cassie
                .execute_sql(
                    &session,
                    &format!(
                        "INSERT INTO read_model_column_batch \
                         (title, body, status, score) VALUES \
                         ('{title}', '{body}', '{status}', {score})"
                    ),
                    vec![],
                )
                .unwrap();
        }
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX read_model_column_batch_idx \
                 ON read_model_column_batch USING column (title, body, status, score) \
                 WITH (segment_size = 2)",
                vec![],
            )
            .unwrap();
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title, body FROM read_model_column_batch \
                 WHERE status = 'approved' ORDER BY title",
                vec![],
            )
            .unwrap();
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title, body FROM read_model_column_batch \
                 WHERE status = 'approved' ORDER BY title",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows(),
            vec![
                vec![
                    Value::String("alpha".to_string()),
                    Value::String("one".to_string())
                ],
                vec![
                    Value::String("gamma".to_string()),
                    Value::String("three".to_string())
                ],
            ]
        );
        let plan = explain_plan_text(&explain);
        assert_explain_contains(plan, "column_batch_index", "read_model_column_batch_idx");
        assert!(
            after["column_batches"]["scans"].as_u64().unwrap()
                > before["column_batches"]["scans"].as_u64().unwrap()
        );
        assert!(
            after["column_batches"]["row_fetches_avoided"]
                .as_u64()
                .unwrap()
                >= before["column_batches"]["row_fetches_avoided"]
                    .as_u64()
                    .unwrap()
                    + 2
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_lock_vectorized_join_hot_path_when_enabled() {
    // Arrange
    let path = data_dir("read_model_vectorized_join");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_in_memory_with_config(vectorized_join_config()).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE read_model_join_users (user_key INT, name TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE read_model_join_orders (order_user_key INT, total INT)",
                vec![],
            )
            .unwrap();
        for sql in [
            "INSERT INTO read_model_join_users (user_key, name) VALUES (1, 'ada')",
            "INSERT INTO read_model_join_users (user_key, name) VALUES (2, 'grace')",
            "INSERT INTO read_model_join_orders (order_user_key, total) VALUES (1, 42)",
            "INSERT INTO read_model_join_orders (order_user_key, total) VALUES (2, 7)",
        ] {
            cassie.execute_sql(&session, sql, vec![]).unwrap();
        }
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT read_model_join_users.name, read_model_join_orders.total \
                 FROM read_model_join_users JOIN read_model_join_orders \
                 ON read_model_join_users.user_key = read_model_join_orders.order_user_key \
                 ORDER BY read_model_join_users.name",
                vec![],
            )
            .unwrap();
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT read_model_join_users.name, read_model_join_orders.total \
                 FROM read_model_join_users JOIN read_model_join_orders \
                 ON read_model_join_users.user_key = read_model_join_orders.order_user_key",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows(),
            vec![
                vec![Value::String("ada".to_string()), Value::Int64(42)],
                vec![Value::String("grace".to_string()), Value::Int64(7)],
            ]
        );
        let plan = explain_plan_text(&explain);
        assert_explain_contains(plan, "vectorized_join_candidate", "true");
        assert_explain_contains(plan, "vectorized_join_enabled", "true");
        assert_explain_contains(plan, "vectorized_join_fallback_reason", "none");
        assert!(
            after["joins"]["vectorized_joins"].as_u64().unwrap()
                > before["joins"]["vectorized_joins"].as_u64().unwrap()
        );
        assert_eq!(after["joins"]["last_strategy"].as_str(), Some("typed_hash"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_lock_pgwire_prepared_read_hot_path_metrics() {
    // Arrange
    let path = data_dir("read_model_pgwire_prepared");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.password = "postgres".to_string();
        let cassie = Cassie::new_in_memory_with_config(config.clone()).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE read_model_pgwire_prepared (title TEXT, score INT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO read_model_pgwire_prepared (title, score) VALUES ('alpha', 7)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX read_model_pgwire_prepared_score_idx \
                 ON read_model_pgwire_prepared USING btree (score)",
                vec![],
            )
            .unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener address");
        drop(listener);

        let server = tokio::spawn(cassie::pgwire::server::run(
            addr.to_string(),
            Arc::new(cassie.clone()),
            config,
        ));
        tokio::time::sleep(Duration::from_millis(50)).await;

        let before = cassie.metrics();
        let mut client_config = tokio_postgres::Config::new();
        client_config.host("127.0.0.1");
        client_config.port(addr.port());
        client_config.user("root");
        client_config.password("postgres");
        client_config.dbname("postgres");
        let (client, connection) = client_config
            .connect(NoTls)
            .await
            .expect("connect tokio-postgres");
        let connection = tokio::spawn(async move {
            connection
                .await
                .expect("tokio-postgres connection should stay healthy");
        });

        // Act
        let statement = client
            .prepare("SELECT title FROM read_model_pgwire_prepared WHERE score = $1")
            .await
            .expect("prepare statement");
        let row = client
            .query_one(&statement, &[&7_i32])
            .await
            .expect("execute prepared statement");
        let title: String = row.try_get(0).expect("title value");
        drop(client);
        let _ = connection.await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        let after = cassie.metrics();

        // Assert
        assert_eq!(title, "alpha");
        assert!(
            after["pgwire"]["extended_queries_total"].as_u64().unwrap()
                > before["pgwire"]["extended_queries_total"].as_u64().unwrap()
        );
        assert_eq!(
            after["pgwire"]["protocol_errors_total"].as_u64(),
            before["pgwire"]["protocol_errors_total"].as_u64()
        );
        assert!(
            after["read_paths"]["index_seek_scans"].as_u64().unwrap()
                > before["read_paths"]["index_seek_scans"].as_u64().unwrap()
        );

        server.abort();
        let _ = server.await;
        let _ = std::fs::remove_dir_all(path);
    });
}
