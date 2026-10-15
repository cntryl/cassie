use cassie::app::CassieError;
use cassie::catalog::{canonical_relation_name, canonical_schema_name, Catalog};
use cassie::sql::ast::{QuerySource, QueryStatement};
use cassie::sql::binder::{bind_with_context, BindingContext};
use cassie::sql::parse_statement;
use cassie::types::{DataType, FieldSchema, Schema};

#[test]
fn should_parse_create_plus_drop_database_statements() {
    // Arrange
    let create_sql = "CREATE DATABASE tenant_b";
    let drop_sql = "DROP DATABASE IF EXISTS tenant_b";

    // Act
    let create = parse_statement(create_sql).expect("create database should parse");
    let drop = parse_statement(drop_sql).expect("drop database should parse");

    // Assert
    assert!(matches!(
        create.statement,
        QueryStatement::CreateDatabase(_)
    ));
    assert!(matches!(drop.statement, QueryStatement::DropDatabase(_)));
}

#[test]
fn should_bind_unqualified_names_through_search_path() {
    // Arrange
    let catalog = Catalog::new();
    catalog.register_database("postgres", None);
    catalog.register_namespace(&canonical_schema_name("postgres", "public"), None);
    catalog.register_namespace(&canonical_schema_name("postgres", "reporting"), None);
    catalog.register_collection(
        &canonical_relation_name("postgres", "public", "orders"),
        Schema {
            fields: vec![FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            }],
        }
        .fields
        .into_iter()
        .map(|field| (field.name, field.data_type))
        .collect(),
    );
    catalog.register_collection(
        &canonical_relation_name("postgres", "reporting", "orders"),
        vec![("title".to_string(), DataType::Text)],
    );
    let context = BindingContext::scoped(
        "postgres",
        vec!["reporting".to_string(), "public".to_string()],
    );
    let parsed = parse_statement("SELECT title FROM orders").expect("select should parse");

    // Act
    let bound = bind_with_context(parsed, &catalog, &context).expect("bind should succeed");

    // Assert
    let QueryStatement::Select(select) = bound.statement.statement else {
        panic!("expected SELECT");
    };
    let QuerySource::Collection(name) = select.source else {
        panic!("expected collection source");
    };
    assert_eq!(
        name,
        canonical_relation_name("postgres", "reporting", "orders")
    );
}

#[test]
fn should_reject_cross_database_relation_references() {
    // Arrange
    let catalog = Catalog::new();
    let context = BindingContext::scoped("postgres", vec!["public".to_string()]);
    let parsed =
        parse_statement("SELECT title FROM tenant_b.public.orders").expect("select should parse");

    // Act
    let error =
        bind_with_context(parsed, &catalog, &context).expect_err("cross-database bind should fail");

    // Assert
    let CassieError::Unsupported(message) = error else {
        panic!("expected unsupported cross-database error");
    };
    assert!(message.contains("cross-database relation references are not supported"));
}
