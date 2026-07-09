#![allow(unused_imports)]

use cassie::app::Cassie;
use cassie::catalog::{CollectionStorageMode, IndexKind, IndexMeta};
use cassie::sql::ast::{
    BinaryOp, CteQuery, Expr, InsertSource, JoinKind, QuerySource, QueryStatement, SelectItem,
    SetOperator, SortDirection,
};
use cassie::sql::parse_statement;
use cassie::types::{DataType, FieldSchema, Schema};
use std::collections::BTreeMap;
use uuid::Uuid;

#[test]
fn should_parse_with_clause_with_cte_source() {
    // Arrange
    let sql = "WITH docs_cte AS (SELECT title FROM docs) SELECT title FROM docs_cte";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    assert_eq!(statement.ctes.len(), 1);
    assert_eq!(statement.ctes[0].name, "docs_cte");
    assert!(matches!(statement.ctes[0].query, CteQuery::Simple(_)));
    assert_eq!(
        statement.source,
        QuerySource::Collection("docs_cte".to_string())
    );
}

#[test]
fn should_parse_multiple_ctes_with_dependencies() {
    // Arrange
    let sql = "WITH first AS (SELECT title FROM docs), second AS (SELECT title FROM first) SELECT title FROM second";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    assert_eq!(statement.ctes.len(), 2);
    assert_eq!(statement.ctes[0].name, "first");
    assert_eq!(statement.ctes[1].name, "second");
    assert_eq!(
        statement.source,
        QuerySource::Collection("second".to_string())
    );
}

#[test]
fn should_parse_recursive_cte_shape() {
    // Arrange
    let sql = "with recursive counter(n) as (SELECT n FROM docs UNION ALL SELECT n FROM counter) SELECT n FROM counter";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    assert!(statement.recursive);
    assert_eq!(statement.ctes.len(), 1);
    assert_eq!(statement.ctes[0].aliases, vec!["n".to_string()]);
    assert!(matches!(
        statement.ctes[0].query,
        CteQuery::Recursive { .. }
    ));
}

#[test]
fn should_parse_cte_column_aliases() {
    // Arrange
    let sql =
        "WITH docs_cte(title_alias) AS (SELECT title FROM docs) SELECT title_alias FROM docs_cte";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    assert_eq!(statement.ctes[0].aliases.len(), 1);
    assert_eq!(statement.ctes[0].aliases[0], "title_alias");
    assert_eq!(
        statement.source,
        QuerySource::Collection("docs_cte".to_string())
    );
}

#[test]
fn should_parse_create_table_with_if_not_exists() {
    // Arrange
    let sql =
        "CREATE TABLE IF NOT EXISTS users (id INT, title TEXT, embedding VECTOR(3), flag BOOLEAN)";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::CreateTable(statement) = parsed.statement else {
        panic!("expected create table statement");
    };

    assert_eq!(statement.table, "users");
    assert!(statement.if_not_exists);
    assert_eq!(statement.fields.len(), 4);
    assert_eq!(statement.fields[1].name, "title");
    assert_eq!(statement.fields[1].data_type, DataType::Text);
}

#[test]
fn should_parse_create_graph_field_sections() {
    // Arrange
    let sql = "CREATE GRAPH IF NOT EXISTS knowledge (NODES (label TEXT, embedding VECTOR(2)), EDGES (source TEXT))";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::CreateGraph(statement) = parsed.statement else {
        panic!("expected create graph statement");
    };
    assert_eq!(statement.name, "knowledge");
    assert!(statement.if_not_exists);
    assert_eq!(statement.node_fields.len(), 2);
    assert_eq!(statement.node_fields[0].name, "label");
    assert_eq!(statement.node_fields[1].data_type, DataType::Vector(2));
    assert_eq!(statement.edge_fields.len(), 1);
    assert_eq!(statement.edge_fields[0].name, "source");
}

#[test]
fn should_parse_graph_table_function_source() {
    // Arrange
    let sql =
        "SELECT node_id FROM graph_expand('knowledge', 'person', 'alice', 2, 'out', 'knows', 10)";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };
    let QuerySource::TableFunction {
        name,
        function,
        lateral,
    } = statement.source
    else {
        panic!("expected table function source");
    };
    assert_eq!(name, "graph_expand");
    assert_eq!(function.args.len(), 7);
    assert!(!lateral);
}

#[test]
fn should_parse_create_table_with_column_store_storage_mode() {
    // Arrange
    let sql = "CREATE TABLE analytics_docs (id TEXT, title TEXT) WITH (storage = column_store)";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::CreateTable(statement) = parsed.statement else {
        panic!("expected create table statement");
    };

    assert_eq!(statement.table, "analytics_docs");
    assert_eq!(statement.storage_mode, CollectionStorageMode::ColumnStore);
}

#[test]
fn should_reject_create_table_with_column_indexed_storage_mode() {
    // Arrange
    let sql = "CREATE TABLE analytics_docs (id TEXT) WITH (storage = column_indexed)";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(matches!(
        parsed,
        Err(err) if err.message().contains("column_indexed")
    ));
}

#[test]
fn should_parse_drop_table_with_if_exists() {
    // Arrange
    let sql = "DROP TABLE IF EXISTS users";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::DropTable(statement) = parsed.statement else {
        panic!("expected drop table statement");
    };

    assert_eq!(statement.table, "users");
    assert!(statement.if_exists);
}

#[test]
fn should_parse_create_schema_if_not_exists() {
    // Arrange
    let sql = "CREATE SCHEMA IF NOT EXISTS reporting";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::CreateSchema(statement) = parsed.statement else {
        panic!("expected create schema statement");
    };

    assert_eq!(statement.schema, "reporting");
    assert!(statement.if_not_exists);
}

#[test]
fn should_parse_drop_schema_statement() {
    // Arrange
    let sql = "DROP SCHEMA IF EXISTS reporting";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::DropSchema(statement) = parsed.statement else {
        panic!("expected drop schema statement");
    };

    assert_eq!(statement.schema, "reporting");
    assert!(statement.if_exists);
}

#[test]
fn should_parse_alter_schema_rename_statement() {
    // Arrange
    let sql = "ALTER SCHEMA reporting RENAME TO reporting_archive";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::AlterSchema(statement) = parsed.statement else {
        panic!("expected alter schema statement");
    };

    match statement.operation {
        cassie::sql::ast::AlterSchemaOperation::RenameTo { schema } => {
            assert_eq!(schema, "reporting_archive");
        }
    }
}

#[test]
fn should_reject_create_schema_when_schema_exists_without_if_not_exists() {
    // Arrange
    let cassie =
        Cassie::new_with_data_dir(format!("/tmp/cassie-parser-schema-{}", Uuid::new_v4())).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie
            .catalog
            .register_namespace("reporting", None);

        // Act
        let parsed = parse_statement("CREATE SCHEMA reporting")
            .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

        // Assert
        assert!(matches!(bound, Err(err) if err.to_string().contains("namespace 'reporting' already exists")));
    });
}

#[test]
fn should_parse_rename_table_alter_statement() {
    // Arrange
    let sql = "ALTER TABLE docs RENAME TO docs_archive";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::AlterTable(statement) = parsed.statement else {
        panic!("expected alter table statement");
    };

    assert_eq!(statement.table, "docs");
    match statement.operation {
        cassie::sql::ast::AlterTableOperation::RenameTo { table } => {
            assert_eq!(table, "docs_archive");
        }
        _ => panic!("expected rename operation"),
    }
}

#[test]
fn should_parse_rename_column_alter_statement() {
    // Arrange
    let sql = "ALTER TABLE docs RENAME COLUMN title TO headline";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::AlterTable(statement) = parsed.statement else {
        panic!("expected alter table statement");
    };

    assert_eq!(statement.table, "docs");
    match statement.operation {
        cassie::sql::ast::AlterTableOperation::RenameColumn { from, to } => {
            assert_eq!(from, "title");
            assert_eq!(to, "headline");
        }
        _ => panic!("expected rename column operation"),
    }
}

#[test]
fn should_reject_duplicate_fields_in_create_table_definition() {
    // Arrange
    let sql = "CREATE TABLE dup_cols (id TEXT, id TEXT)";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_err());
}

#[test]
fn should_parse_create_table_field_constraints() {
    // Arrange
    let sql =
        "CREATE TABLE users (id INT PRIMARY KEY, email TEXT NOT NULL UNIQUE DEFAULT 'anon', score INT CHECK (score >= 0))";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::CreateTable(statement) = parsed.statement else {
        panic!("expected create table statement");
    };
    assert_eq!(statement.table, "users");
    assert_eq!(statement.fields.len(), 3);

    assert!(statement.fields[0]
        .constraints
        .iter()
        .any(|c| c.primary_key));
    assert!(!statement.fields[0].constraints.iter().any(|c| c.unique));

    let email_constraints = &statement.fields[1].constraints;
    assert_eq!(email_constraints.len(), 1);
    assert!(email_constraints[0].not_null);
    assert!(email_constraints[0].unique);
    assert_eq!(
        email_constraints[0].default_value,
        Some(serde_json::Value::String("anon".to_string()))
    );

    let score_constraints = &statement.fields[2].constraints;
    assert_eq!(score_constraints.len(), 1);
    assert!(score_constraints[0].check.is_some());
}

#[test]
fn should_parse_create_table_foreign_key_constraint() {
    // Arrange
    let sql = "CREATE TABLE orders (customer_id INT REFERENCES customers(id))";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::CreateTable(statement) = parsed.statement else {
        panic!("expected create table statement");
    };
    let constraints = &statement.fields[0].constraints;
    assert_eq!(constraints.len(), 1);
    assert_eq!(
        constraints[0].references_table.as_deref(),
        Some("customers")
    );
    assert_eq!(constraints[0].references_field.as_deref(), Some("id"));
}

#[test]
fn should_reject_create_table_constraints_without_parentheses() {
    // Arrange
    let sql = "CREATE TABLE broken (id INT CHECK score >= 0)";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_err());
}

#[test]
fn should_reject_reserved_namespace_on_create_schema() {
    // Arrange
    let cassie = Cassie::new_with_data_dir(format!(
        "/tmp/cassie-parser-reserved-schema-{}",
        Uuid::new_v4()
    ))
    .unwrap();

    // Act
    let parsed = parse_statement("CREATE SCHEMA public").expect("parse should succeed");

    // Assert
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

        assert!(bound.is_err());
    });
}

#[test]
fn should_reject_reserved_namespace_on_drop_schema() {
    // Arrange
    let cassie = Cassie::new_with_data_dir(format!(
        "/tmp/cassie-parser-reserved-drop-schema-{}",
        Uuid::new_v4()
    ))
    .unwrap();

    // Act
    let parsed = parse_statement("DROP SCHEMA public").expect("parse should succeed");

    // Assert
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

        assert!(bound.is_err());
    });
}

#[test]
fn should_reject_reserved_namespace_on_alter_schema() {
    // Arrange
    let cassie = Cassie::new_with_data_dir(format!(
        "/tmp/cassie-parser-reserved-alter-schema-{}",
        Uuid::new_v4()
    ))
    .unwrap();

    // Act
    let parsed =
        parse_statement("ALTER SCHEMA public RENAME TO archive").expect("parse should succeed");

    // Assert
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

        assert!(bound.is_err());
    });
}

#[test]
fn should_reject_create_table_when_collection_exists_without_if_not_exists() {
    // Arrange
    let cassie =
        Cassie::new_with_data_dir(format!("/tmp/cassie-parser-{}", Uuid::new_v4())).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie.register_collection(
            "existing_table".to_string(),
            Schema {
                fields: vec![FieldSchema {
                    name: "id".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            },
        );

        // Act
        let parsed = parse_statement("CREATE TABLE existing_table (title TEXT)")
            .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

        // Assert
        assert!(bound.is_err());
    });
}

#[test]
fn should_reject_drop_table_when_collection_missing_without_if_exists() {
    // Arrange
    let cassie =
        Cassie::new_with_data_dir(format!("/tmp/cassie-parser-{}", Uuid::new_v4())).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        // Act
        let parsed = parse_statement("DROP TABLE missing_table").expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

        // Assert
        assert!(bound.is_err());
    });
}
