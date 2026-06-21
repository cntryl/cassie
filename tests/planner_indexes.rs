#![allow(unused_imports, dead_code)]
use cassie::app::CassieError;
use cassie::catalog::{Catalog, IndexKind, IndexMeta};
use cassie::planner::{logical, optimizer, physical, physical::Operator};
use cassie::sql::ast::{
    BinaryOp, Expr, InsertSource, JoinKind, ParsedStatement, QuerySource, QueryStatement,
    SelectItem, SelectStatement, SortDirection,
};
use cassie::sql::binder::BoundStatement;
use cassie::sql::{binder, parser};
use cassie::types::{DataType, FieldSchema};
use std::collections::BTreeMap;

fn register_test_collection(catalog: &Catalog, name: &str) {
    let schema = vec![
        FieldSchema {
            name: "title".to_string(),
            data_type: DataType::Text,
            nullable: true,
        },
        FieldSchema {
            name: "body".to_string(),
            data_type: DataType::Text,
            nullable: true,
        },
    ];

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        catalog.register_collection(
            name,
            schema
                .into_iter()
                .map(|field| (field.name, field.data_type))
                .collect(),
        );
    });
}

fn register_scalar_index(catalog: &Catalog, collection: &str, name: &str, fields: Vec<&str>) {
    let fields = fields
        .into_iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    catalog.register_index(IndexMeta {
        collection: collection.to_string(),
        name: name.to_string(),
        field: fields.first().cloned().unwrap_or_default(),
        fields,
        expressions: Vec::new(),
        include_fields: Vec::new(),
        predicate: None,
        kind: IndexKind::Scalar,
        unique: false,
        options: BTreeMap::new(),
    });
}

#[test]
fn should_select_scalar_index_for_equality_filter() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_index_aware");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        catalog.register_index(cassie::catalog::IndexMeta {
            collection: "planner_index_aware".to_string(),
            name: "planner_index_aware_title_idx".to_string(),
            field: "title".to_string(),
            fields: vec!["title".to_string()],
            expressions: Vec::new(),
            include_fields: Vec::new(),
            predicate: None,
            kind: cassie::catalog::IndexKind::Scalar,
            unique: false,
            options: Default::default(),
        });
        let parsed =
            parser::parse_statement("SELECT body FROM planner_index_aware WHERE title = 'alpha'")
                .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();
        let logical = optimizer::optimize(logical);

        // Act
        let cardinality_stats = std::collections::HashMap::new();
        let physical_plan =
            physical::build_with_indexes(logical, bound.indexes, &cardinality_stats);

        // Assert
        assert_eq!(
            physical_plan.selected_index.as_deref(),
            Some("planner_index_aware_title_idx")
        );
    });
}

#[test]
fn should_mark_scalar_index_plan_as_covered() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_covering_index");
    register_scalar_index(
        &catalog,
        "planner_covering_index",
        "planner_covering_title_idx",
        vec!["title"],
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT title FROM planner_covering_index WHERE title = 'alpha' ORDER BY title",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();

        // Act
        let physical_plan = physical::build_with_indexes(
            logical,
            catalog.list_indexes("planner_covering_index"),
            &Default::default(),
        );

        // Assert
        assert_eq!(
            physical_plan.selected_index.as_deref(),
            Some("planner_covering_title_idx")
        );
        assert!(physical_plan.covered_index);
    });
}

#[test]
fn should_leave_noncovered_scalar_index_plan_uncovered() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_covering_fallback");
    register_scalar_index(
        &catalog,
        "planner_covering_fallback",
        "planner_covering_fallback_title_idx",
        vec!["title"],
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT body FROM planner_covering_fallback WHERE title = 'alpha'",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();

        // Act
        let physical_plan = physical::build_with_indexes(
            logical,
            catalog.list_indexes("planner_covering_fallback"),
            &Default::default(),
        );

        // Assert
        assert_eq!(
            physical_plan.selected_index.as_deref(),
            Some("planner_covering_fallback_title_idx")
        );
        assert!(!physical_plan.covered_index);
    });
}

#[test]
fn should_mark_include_column_plan_as_covered() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_include_covering");
    catalog.register_index(IndexMeta {
        collection: "planner_include_covering".to_string(),
        name: "planner_include_covering_title_idx".to_string(),
        field: "title".to_string(),
        fields: vec!["title".to_string()],
        expressions: Vec::new(),
        include_fields: vec!["body".to_string()],
        predicate: None,
        kind: IndexKind::Scalar,
        unique: false,
        options: BTreeMap::new(),
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT body FROM planner_include_covering WHERE title = 'alpha'",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();

        // Act
        let physical_plan = physical::build_with_indexes(
            logical,
            catalog.list_indexes("planner_include_covering"),
            &Default::default(),
        );

        // Assert
        assert_eq!(
            physical_plan.selected_index.as_deref(),
            Some("planner_include_covering_title_idx")
        );
        assert!(physical_plan.covered_index);
    });
}

#[test]
fn should_select_partial_index_for_exact_predicate() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_partial_index");
    let predicate =
        parser::parse_statement("SELECT title FROM planner_partial_index WHERE title = 'alpha'")
            .and_then(|parsed| match parsed.statement {
                cassie::sql::ast::QueryStatement::Select(select) => {
                    Ok(select.filter.expect("filter"))
                }
                _ => unreachable!(),
            })
            .unwrap();
    catalog.register_index(IndexMeta {
        collection: "planner_partial_index".to_string(),
        name: "planner_partial_index_title_idx".to_string(),
        field: "title".to_string(),
        fields: vec!["title".to_string()],
        expressions: Vec::new(),
        include_fields: Vec::new(),
        predicate: Some(serde_json::to_string(&predicate).unwrap()),
        kind: IndexKind::Scalar,
        unique: false,
        options: BTreeMap::new(),
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT title FROM planner_partial_index WHERE title = 'alpha'",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();

        // Act
        let physical_plan = physical::build_with_indexes(
            logical,
            catalog.list_indexes("planner_partial_index"),
            &Default::default(),
        );

        // Assert
        assert_eq!(
            physical_plan.selected_index.as_deref(),
            Some("planner_partial_index_title_idx")
        );
    });
}

#[test]
fn should_skip_partial_index_for_unsafe_predicate() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_partial_fallback");
    catalog.register_index(IndexMeta {
        collection: "planner_partial_fallback".to_string(),
        name: "planner_partial_fallback_title_idx".to_string(),
        field: "title".to_string(),
        fields: vec!["title".to_string()],
        expressions: Vec::new(),
        include_fields: Vec::new(),
        predicate: Some("{\"Column\":\"status\"}".to_string()),
        kind: IndexKind::Scalar,
        unique: false,
        options: BTreeMap::new(),
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT title FROM planner_partial_fallback WHERE title = 'alpha'",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();

        // Act
        let physical_plan = physical::build_with_indexes(
            logical,
            catalog.list_indexes("planner_partial_fallback"),
            &Default::default(),
        );

        // Assert
        assert!(physical_plan.selected_index.is_none());
    });
}

#[test]
fn should_select_expression_index_for_matching_predicate() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_expression_index");
    let expression = parser::parse_statement(
        "CREATE INDEX planner_expression_index_lower_idx ON planner_expression_index USING btree (lower(title))",
    )
    .and_then(|parsed| match parsed.statement {
        QueryStatement::CreateIndex(statement) => Ok(statement.expressions[0].clone()),
        _ => unreachable!(),
    })
    .unwrap();
    catalog.register_index(IndexMeta {
        collection: "planner_expression_index".to_string(),
        name: "planner_expression_index_lower_idx".to_string(),
        field: String::new(),
        fields: Vec::new(),
        expressions: vec![serde_json::to_string(&expression).unwrap()],
        include_fields: Vec::new(),
        predicate: None,
        kind: IndexKind::Scalar,
        unique: false,
        options: BTreeMap::new(),
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT title FROM planner_expression_index WHERE lower(title) = 'alpha'",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();

        // Act
        let physical_plan = physical::build_with_indexes(
            logical,
            catalog.list_indexes("planner_expression_index"),
            &Default::default(),
        );

        // Assert
        assert_eq!(
            physical_plan.selected_index.as_deref(),
            Some("planner_expression_index_lower_idx")
        );
    });
}

#[test]
fn should_skip_expression_index_for_non_equivalent_predicate() {
    // Arrange
    let catalog = Catalog::new();
    register_test_collection(&catalog, "planner_expression_fallback");
    let expression = parser::parse_statement(
        "CREATE INDEX planner_expression_fallback_lower_idx ON planner_expression_fallback USING btree (lower(title))",
    )
    .and_then(|parsed| match parsed.statement {
        QueryStatement::CreateIndex(statement) => Ok(statement.expressions[0].clone()),
        _ => unreachable!(),
    })
    .unwrap();
    catalog.register_index(IndexMeta {
        collection: "planner_expression_fallback".to_string(),
        name: "planner_expression_fallback_lower_idx".to_string(),
        field: String::new(),
        fields: Vec::new(),
        expressions: vec![serde_json::to_string(&expression).unwrap()],
        include_fields: Vec::new(),
        predicate: None,
        kind: IndexKind::Scalar,
        unique: false,
        options: BTreeMap::new(),
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let parsed = parser::parse_statement(
            "SELECT title FROM planner_expression_fallback WHERE title = 'alpha'",
        )
        .unwrap();
        let bound = binder::bind(parsed, &catalog).unwrap();
        let logical = logical::plan(&bound).unwrap();

        // Act
        let physical_plan = physical::build_with_indexes(
            logical,
            catalog.list_indexes("planner_expression_fallback"),
            &Default::default(),
        );

        // Assert
        assert!(physical_plan.selected_index.is_none());
    });
}
