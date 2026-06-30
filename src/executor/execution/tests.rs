use super::*;

fn plan_for_sql(sql: &str) -> LogicalPlan {
    let parsed = crate::sql::parse_statement(sql).expect("parse statement");
    build_logical_plan(&parsed).expect("build logical plan")
}

fn scalar_index(collection: &str, name: &str, fields: Vec<&str>) -> catalog::IndexMeta {
    catalog::IndexMeta {
        collection: collection.to_string(),
        name: name.to_string(),
        field: fields.first().copied().unwrap_or_default().to_string(),
        fields: fields.into_iter().map(str::to_string).collect(),
        expressions: Vec::new(),
        include_fields: Vec::new(),
        predicate: None,
        kind: catalog::IndexKind::Scalar,
        unique: false,
        options: std::collections::BTreeMap::default(),
    }
}

#[test]
fn should_route_scalar_physical_read_paths_directly_to_scalar_index_executor() {
    // Arrange
    let logical = plan_for_sql(
        "SELECT id FROM bench_documents \
         WHERE status = 'approved' AND score >= 10 \
         ORDER BY status DESC, score ASC LIMIT 50",
    );
    let physical = crate::planner::physical::build_with_indexes(
        logical,
        &[scalar_index(
            "bench_documents",
            "bench_documents_status_score_idx",
            vec!["status", "score"],
        )],
        &std::collections::HashMap::<String, crate::catalog::CollectionCardinalityStats>::default(),
    );
    assert_eq!(
        physical.access_path,
        crate::planner::physical::ReadAccessPath::RangeScan
    );

    // Act
    let route = preferred_access_path_route(Some(&physical));

    // Assert
    assert_eq!(route, Some(AccessPathRoute::ScalarIndex));
}

#[test]
fn should_detect_unordered_fulltext_fast_path_for_matching_search_query() {
    // Arrange
    let plan = plan_for_sql(
            "SELECT id, search_score(body, 'alpha') AS score FROM bench_documents WHERE search(body, 'alpha')",
        );

    // Act
    let spec = scored::fulltext_filtered_read_spec(&plan);

    // Assert
    let spec = spec.expect("unordered fulltext fast path");
    assert_eq!(spec.collection, "bench_documents");
    assert_eq!(spec.text_field, "body");
    assert_eq!(spec.query, "alpha");
    assert_eq!(spec.score_column, "score");
    assert_eq!(spec.columns.len(), 1);
    assert_eq!(spec.columns[0].name, "id");
    assert_eq!(spec.columns[0].output_name, "id");
}

#[test]
fn should_reject_unordered_fulltext_fast_path_for_mismatched_search_query() {
    // Arrange
    let plan = plan_for_sql(
            "SELECT id, search_score(body, 'alpha') AS score FROM bench_documents WHERE search(body, 'bravo')",
        );

    // Act
    let spec = scored::fulltext_filtered_read_spec(&plan);

    // Assert
    assert!(spec.is_none());
}

#[test]
fn should_reject_unordered_fulltext_fast_path_for_additional_filters() {
    // Arrange
    let plan = plan_for_sql(
            "SELECT id, search_score(body, 'alpha') AS score FROM bench_documents WHERE search(body, 'alpha') AND status = 'approved'",
        );

    // Act
    let spec = scored::fulltext_filtered_read_spec(&plan);

    // Assert
    assert!(spec.is_none());
}

#[test]
fn should_reject_unordered_fulltext_fast_path_for_wildcard_projection() {
    // Arrange
    let plan = plan_for_sql("SELECT * FROM bench_documents WHERE search(body, 'alpha')");

    // Act
    let spec = scored::fulltext_filtered_read_spec(&plan);

    // Assert
    assert!(spec.is_none());
}

#[test]
fn should_build_projected_read_spec_without_filter() {
    // Arrange
    let plan = plan_for_sql("SELECT id, title FROM bench_documents LIMIT 20");

    // Act
    let spec = projected_read::projected_filtered_read_spec(&plan);

    // Assert
    let spec = spec.expect("projected read spec");
    assert_eq!(spec.collection, "bench_documents");
    assert_eq!(spec.scan_fields, vec!["title".to_string()]);
}

#[test]
fn should_push_limit_into_projected_read_spec_without_filter() {
    // Arrange
    let plan = plan_for_sql("SELECT id, title FROM bench_documents LIMIT 20");

    // Act
    let spec = projected_read::projected_filtered_read_spec(&plan);

    // Assert
    let spec = spec.expect("projected read spec");
    assert_eq!(spec.scan_limit, Some(20));
}

#[test]
fn should_include_offset_in_projected_read_spec_scan_limit() {
    // Arrange
    let plan = plan_for_sql("SELECT id, title FROM bench_documents LIMIT 20 OFFSET 5");

    // Act
    let spec = projected_read::projected_filtered_read_spec(&plan);

    // Assert
    let spec = spec.expect("projected read spec");
    assert_eq!(spec.scan_limit, Some(25));
}

#[test]
fn should_not_push_limit_into_projected_read_spec_when_filter_is_present() {
    // Arrange
    let plan =
        plan_for_sql("SELECT id, title FROM bench_documents WHERE status = 'approved' LIMIT 20");

    // Act
    let spec = projected_read::projected_filtered_read_spec(&plan);

    // Assert
    let spec = spec.expect("projected read spec");
    assert_eq!(spec.scan_limit, None);
}

#[test]
fn should_detect_projected_scan_pushdown_for_literal_equality() {
    // Arrange
    let plan = plan_for_sql("SELECT id, title FROM bench_documents WHERE title = 'alpha'");
    let filter = plan.filter.as_ref().expect("filter");

    // Act
    let pushdown = projected_read::projected_scan_pushdown_filter(filter);

    // Assert
    let pushdown = pushdown.expect("pushdown filter");
    assert_eq!(pushdown.field, "title");
    assert_eq!(pushdown.value, Value::String("alpha".to_string()));
}

#[test]
fn should_reject_projected_scan_pushdown_for_row_id_equality() {
    // Arrange
    let plan = plan_for_sql("SELECT id, title FROM bench_documents WHERE id = 'doc-1'");
    let filter = plan.filter.as_ref().expect("filter");

    // Act
    let pushdown = projected_read::projected_scan_pushdown_filter(filter);

    // Assert
    assert!(pushdown.is_none());
}

#[test]
fn should_skip_user_function_catalog_for_builtin_only_plan() {
    // Arrange
    let plan =
        plan_for_sql("SELECT id FROM bench_documents WHERE score >= 10 ORDER BY id LIMIT 20");

    // Act
    let needs_user_functions = plan_needs_user_functions(&plan);

    // Assert
    assert!(!needs_user_functions);
}

#[test]
fn should_require_user_function_catalog_for_user_defined_function_plan() {
    // Arrange
    let plan =
        plan_for_sql("SELECT my_udf(title) AS normalized_title FROM bench_documents LIMIT 20");

    // Act
    let needs_user_functions = plan_needs_user_functions(&plan);

    // Assert
    assert!(needs_user_functions);
}

#[test]
fn should_report_execution_breakdown_for_projected_filtered_read() {
    // Arrange
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
    let mut path = std::env::temp_dir();
    path.push(format!(
        "cassie-execution-breakdown-{}",
        uuid::Uuid::new_v4()
    ));
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    let collection = "breakdown_documents";
    let schema = Schema {
        fields: vec![FieldSchema {
            name: "title".to_string(),
            data_type: DataType::Text,
            nullable: true,
        }],
    };
    cassie
        .midge
        .create_collection(collection, schema.clone())
        .expect("create collection");
    cassie.register_collection(collection, schema);
    cassie
        .midge
        .put_document(
            collection,
            Some("doc-1".to_string()),
            serde_json::json!({"title": "alpha"}),
        )
        .expect("put document");

    let logical = plan_for_sql("SELECT id, title FROM breakdown_documents WHERE title = 'alpha'");
    let physical = crate::planner::physical::build(logical);

    // Act
    let output =
        run_with_execution_breakdown(&cassie, physical, vec![]).expect("execution breakdown");

    // Assert
    assert_eq!(output.result.rows.len(), 1);
    assert_eq!(
        output.result.rows[0],
        vec![
            Value::String("doc-1".to_string()),
            Value::String("alpha".to_string()),
        ]
    );
    assert!(output.breakdown.scan_us > 0 || output.breakdown.row_decode_us > 0);
    assert_eq!(output.breakdown.filter_us, 0);
    assert!(output.breakdown.projection_us > 0);
    assert!(output.breakdown.result_build_us > 0);

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_report_execution_breakdown_for_point_lookup_read() {
    // Arrange
    let mut path = std::env::temp_dir();
    path.push(format!(
        "cassie-execution-point-lookup-{}",
        uuid::Uuid::new_v4()
    ));
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    let collection = "point_lookup_documents";
    let schema = Schema {
        fields: vec![FieldSchema {
            name: "title".to_string(),
            data_type: DataType::Text,
            nullable: true,
        }],
    };
    cassie
        .midge
        .create_collection(collection, schema.clone())
        .expect("create collection");
    cassie.register_collection(collection, schema);
    cassie
        .midge
        .put_document(
            collection,
            Some("doc-1".to_string()),
            serde_json::json!({"title": "alpha"}),
        )
        .expect("put document");

    let logical = plan_for_sql("SELECT id, title FROM point_lookup_documents WHERE id = 'doc-1'");
    let physical = crate::planner::physical::build(logical);

    // Act
    let output =
        run_with_execution_breakdown(&cassie, physical, vec![]).expect("execution breakdown");

    // Assert
    assert_eq!(output.result.rows.len(), 1);
    assert_eq!(
        output.result.rows[0],
        vec![
            Value::String("doc-1".to_string()),
            Value::String("alpha".to_string()),
        ]
    );
    assert_eq!(output.breakdown.scan_us, 0);
    assert_eq!(output.breakdown.filter_us, 0);
    assert_eq!(output.breakdown.sort_us, 0);
    assert!(output.breakdown.result_build_us > 0);

    let _ = std::fs::remove_dir_all(path);
}
