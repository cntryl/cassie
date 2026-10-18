#[test]
fn should_explain_index_aware_plan_for_scalar_equality_filter() {
    // Arrange
    use_local_storage();
    let path = data_dir("explain_index_aware");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);

        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_explain_index_aware (email TEXT, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX sql_explain_index_aware_email_idx ON sql_explain_index_aware USING btree (email)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM sql_explain_index_aware WHERE email = 'a@example.com'",
                vec![],
            )
            .unwrap();

        // Assert
        let Value::String(plan) = &result.rows()[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("index_aware=true"));
        assert!(plan.contains("index=sql_explain_index_aware_email_idx"));
        assert!(plan.contains("access_path=index_seek"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_use_covering_scalar_index_for_projection() {
    // Arrange
    use_local_storage();
    let path = data_dir("covering_scalar_projection");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);

        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_covering_scalar_projection (email TEXT, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO sql_covering_scalar_projection (email, title) VALUES ('a@example.com', 'alpha'), ('b@example.com', 'beta')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX sql_covering_scalar_projection_email_idx ON sql_covering_scalar_projection USING btree (email)",
                vec![],
            )
            .unwrap();
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT email FROM sql_covering_scalar_projection WHERE email = 'a@example.com'",
                vec![],
            )
            .unwrap();
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT email FROM sql_covering_scalar_projection WHERE email = 'a@example.com'",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows(), vec![vec![Value::String("a@example.com".to_string())]]);
        let Value::String(plan) = &explain.rows()[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("index=sql_covering_scalar_projection_email_idx"));
        assert!(plan.contains("covered_index=true"));
        assert_eq!(
            after["covering_indexes"]["scans"]
                .as_u64()
                .unwrap_or_default()
                - before["covering_indexes"]["scans"]
                    .as_u64()
                    .unwrap_or_default(),
            1
        );
        assert_eq!(
            after["covering_indexes"]["row_fetches_avoided"]
                .as_u64()
                .unwrap_or_default()
                - before["covering_indexes"]["row_fetches_avoided"]
                    .as_u64()
                    .unwrap_or_default(),
            1
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_fallback_for_noncovered_scalar_index_projection() {
    // Arrange
    use_local_storage();
    let path = data_dir("covering_scalar_fallback");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);

        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_covering_scalar_fallback (email TEXT, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO sql_covering_scalar_fallback (email, title) VALUES ('a@example.com', 'alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX sql_covering_scalar_fallback_email_idx ON sql_covering_scalar_fallback USING btree (email)",
                vec![],
            )
            .unwrap();
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title FROM sql_covering_scalar_fallback WHERE email = 'a@example.com'",
                vec![],
            )
            .unwrap();
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM sql_covering_scalar_fallback WHERE email = 'a@example.com'",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows(), vec![vec![Value::String("alpha".to_string())]]);
        let Value::String(plan) = &explain.rows()[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("index=sql_covering_scalar_fallback_email_idx"));
        assert!(plan.contains("covered_index=false"));
        assert_eq!(
            after["covering_indexes"]["fallback_scans"]
                .as_u64()
                .unwrap_or_default()
                - before["covering_indexes"]["fallback_scans"]
                    .as_u64()
                    .unwrap_or_default(),
            1
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_use_include_columns_for_covered_projection() {
    // Arrange
    use_local_storage();
    let path = data_dir("include_covered_projection");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE sql_include_covered_projection (email TEXT, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO sql_include_covered_projection (email, title) VALUES ('a@example.com', 'alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX sql_include_covered_projection_email_idx ON sql_include_covered_projection USING btree (email) INCLUDE (title)",
                vec![],
            )
            .unwrap();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title FROM sql_include_covered_projection WHERE email = 'a@example.com'",
                vec![],
            )
            .unwrap();
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM sql_include_covered_projection WHERE email = 'a@example.com'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(result.rows(), vec![vec![Value::String("alpha".to_string())]]);
        let Value::String(plan) = &explain.rows()[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("covered_index=true"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_query_rows_after_creating_secondary_index() {
    // Arrange
    use_local_storage();
    let path = data_dir("secondary_index_query");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE secondary_index_query (email TEXT, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX secondary_email_idx ON secondary_index_query USING btree (email)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO secondary_index_query (email, title) VALUES ('a@example.com', 'alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO secondary_index_query (email, title) VALUES ('b@example.com', 'beta')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM secondary_index_query WHERE email = 'b@example.com'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows(),
            vec![vec![Value::String("beta".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_query_rows_after_creating_composite_index() {
    // Arrange
    use_local_storage();
    let path = data_dir("composite_index_query");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE composite_index_query (tenant_id TEXT, status TEXT, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX composite_tenant_status_idx ON composite_index_query USING btree (tenant_id, status)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO composite_index_query (tenant_id, status, title) VALUES ('tenant-a', 'open', 'alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO composite_index_query (tenant_id, status, title) VALUES ('tenant-a', 'closed', 'beta')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO composite_index_query (tenant_id, status, title) VALUES ('tenant-b', 'closed', 'gamma')",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT title FROM composite_index_query WHERE tenant_id = 'tenant-a' AND status = 'closed'",
                vec![],
            )
            .unwrap();
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM composite_index_query WHERE tenant_id = 'tenant-a' AND status = 'closed'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows(),
            vec![vec![Value::String("beta".to_string())]]
        );
        let Value::String(plan) = &explain.rows()[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("access_path=prefix_scan"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_query_rows_after_creating_range_index() {
    // Arrange
    use_local_storage();
    let path = data_dir("range_index_query");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE range_index_query (title TEXT, body TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX range_title_idx ON range_index_query USING btree (title)",
                vec![],
            )
            .unwrap();
        for (title, body) in [
            ("alpha", "a"),
            ("beta", "b"),
            ("delta", "d"),
            ("omega", "o"),
        ] {
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO range_index_query (title, body) VALUES ($1, $2)",
                    vec![Value::String(title.to_string()), Value::String(body.to_string())],
                )
                .unwrap();
        }

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title FROM range_index_query WHERE title >= 'beta' AND title < 'omega' ORDER BY title ASC",
                vec![],
            )
            .unwrap();
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM range_index_query WHERE title >= 'beta' AND title < 'omega' ORDER BY title ASC",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            result.rows(),
            vec![
                vec![Value::String("beta".to_string())],
                vec![Value::String("delta".to_string())],
            ]
        );
        let Value::String(plan) = &explain.rows()[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("access_path=range_scan"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_page_tenant_filtered_rows_through_composite_range_index() {
    // Arrange
    use_local_storage();
    let path = data_dir("tenant_filtered_page_index_query");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE tenant_filtered_page_index_query (tenant_id TEXT, status TEXT, created_at INT, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX tenant_filtered_page_lookup_idx ON tenant_filtered_page_index_query USING btree (tenant_id, status, created_at)",
                vec![],
            )
            .unwrap();
        for (tenant_id, status, created_at, title) in [
            ("tenant-a", "open", 10, "alpha"),
            ("tenant-a", "open", 20, "beta"),
            ("tenant-a", "open", 30, "gamma"),
            ("tenant-a", "closed", 40, "closed"),
            ("tenant-b", "open", 50, "other"),
        ] {
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO tenant_filtered_page_index_query (tenant_id, status, created_at, title) VALUES ($1, $2, $3, $4)",
                    vec![
                        Value::String(tenant_id.to_string()),
                        Value::String(status.to_string()),
                        Value::Int64(created_at),
                        Value::String(title.to_string()),
                    ],
                )
                .unwrap();
        }
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT title FROM tenant_filtered_page_index_query WHERE tenant_id = 'tenant-a' AND status = 'open' AND created_at >= 10 ORDER BY created_at DESC LIMIT 2",
                vec![],
            )
            .unwrap();
        let explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT title FROM tenant_filtered_page_index_query WHERE tenant_id = 'tenant-a' AND status = 'open' AND created_at >= 10 ORDER BY created_at DESC LIMIT 2",
                vec![],
            )
            .unwrap();
        let after = cassie.metrics();

        // Assert
        assert_eq!(
            result.rows(),
            vec![
                vec![Value::String("gamma".to_string())],
                vec![Value::String("beta".to_string())],
            ]
        );
        let Value::String(plan) = &explain.rows()[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("tenant_filtered_page_lookup_idx"), "plan={plan}");
        assert!(plan.contains("access_path=range_scan"));
        assert!(plan.contains("access_path_reason=scalar-index-range"));
        assert!(plan.contains("pagination_strategy=limit"));
        assert!(plan.contains("early_stop=scan_limit"));
        assert!(plan.contains("fallback_reason=none"));
        assert_eq!(
            after["read_paths"]["range_scans"]
                .as_u64()
                .unwrap_or_default(),
            before["read_paths"]["range_scans"]
                .as_u64()
                .unwrap_or_default()
                + 1,
        );
        assert!(
            after["read_paths"]["last_index_scan_index"]
                .as_str()
                .is_some_and(|value| value.ends_with("tenant_filtered_page_lookup_idx"))
        );

        let _ = std::fs::remove_dir_all(path);
    });
}
